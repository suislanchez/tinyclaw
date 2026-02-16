mod app;
mod markdown;

use crate::channels::build_system_prompt;
use crate::config::Config;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{self, ChatMessage, Provider, UsageTracker};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool};
use crate::session;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Maximum agentic tool-use iterations per user message.
const MAX_TOOL_ITERATIONS: usize = 10;

/// Maximum non-system messages in history.
const MAX_HISTORY_MESSAGES: usize = 50;

/// A token/event from the agent to the TUI
pub enum AgentEvent {
    Token(String),
    ToolStart(String),
    ToolResult { name: String, preview: String },
    Done(String),
    Error(String),
}

/// Run the TUI interface
pub async fn run(
    config: Config,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    // Wire up subsystems
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let runtime_adapter: Arc<dyn runtime::RuntimeAdapter> =
        Arc::from(runtime::create_runtime(&config.runtime)?);
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);

    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools_registry = Arc::new(tools::all_tools_with_runtime(
        &security,
        runtime_adapter,
        mem.clone(),
        composio_key,
        &config.browser,
    ));

    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    let mut provider: Box<dyn Provider> = providers::create_routed_provider(
        provider_name,
        config.api_key.as_deref(),
        &config.reliability,
        &config.model_routes,
        model_name,
    )?;

    let usage_tracker = UsageTracker::new();
    provider.set_usage_tracker(usage_tracker.clone());

    // Build system prompt
    let skills = crate::skills::load_skills(&config.workspace_dir);
    let tool_descs: Vec<(&str, &str)> = vec![
        ("shell", "Execute terminal commands"),
        ("file_read", "Read file contents"),
        ("file_write", "Write file contents"),
        ("memory_store", "Save to memory"),
        ("memory_recall", "Search memory"),
        ("memory_forget", "Delete a memory entry"),
    ];
    let mut system_prompt = build_system_prompt(
        &config.workspace_dir,
        model_name,
        &tool_descs,
        &skills,
        Some(&config.identity),
    );
    system_prompt.push_str(&build_tool_instructions(&tools_registry));

    let history = vec![ChatMessage::system(&system_prompt)];

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    // Bundle agent state and launch TUI
    let agent_state = AgentState {
        provider,
        tools_registry,
        observer,
        mem,
        history,
        model: model_name.to_string(),
        temperature,
        auto_save: config.memory.auto_save,
        usage_tracker: usage_tracker.clone(),
        workspace_dir: config.workspace_dir.clone(),
        session_id: session::new_session_id(),
    };

    app::App::new(model_name.to_string()).run(agent_state).await
}

/// All the state the agent needs between turns, bundled for ownership transfer.
pub struct AgentState {
    pub provider: Box<dyn Provider>,
    pub tools_registry: Arc<Vec<Box<dyn Tool>>>,
    pub observer: Arc<dyn Observer>,
    pub mem: Arc<dyn Memory>,
    pub history: Vec<ChatMessage>,
    pub model: String,
    pub temperature: f64,
    pub auto_save: bool,
    pub usage_tracker: UsageTracker,
    pub workspace_dir: PathBuf,
    pub session_id: String,
}

impl AgentState {
    /// Handle one user message: enrich with memory, run agent loop, send events.
    pub async fn handle_message(
        &mut self,
        user_input: &str,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) {
        // Memory context
        if self.auto_save {
            let _ = self
                .mem
                .store("user_msg", user_input, MemoryCategory::Conversation)
                .await;
        }

        let context = build_context(&*self.mem, user_input).await;
        let enriched = if context.is_empty() {
            user_input.to_string()
        } else {
            format!("{context}{user_input}")
        };

        self.history.push(ChatMessage::user(&enriched));

        let result = agent_turn_with_events(
            &*self.provider,
            &mut self.history,
            &self.tools_registry,
            &*self.observer,
            &self.model,
            self.temperature,
            event_tx,
        )
        .await;

        match result {
            Ok(response) => {
                trim_history(&mut self.history);
                if self.auto_save {
                    let summary = truncate_with_ellipsis(&response, 100);
                    let _ = self
                        .mem
                        .store("assistant_resp", &summary, MemoryCategory::Daily)
                        .await;
                }
                // Auto-save session to disk
                if let Err(e) = session::update(
                    &self.workspace_dir,
                    &self.session_id,
                    &self.model,
                    &self.history,
                ) {
                    tracing::warn!("Failed to save session: {e}");
                }

                let _ = event_tx.send(AgentEvent::Done(response)).await;
            }
            Err(e) => {
                let _ = event_tx.send(AgentEvent::Error(format!("{e:#}"))).await;
            }
        }
    }
}

/// Build context from memory
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    let mut context = String::new();
    if let Ok(entries) = mem.recall(user_msg, 5).await {
        if !entries.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &entries {
                let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
            }
            context.push('\n');
        }
    }
    context
}

/// Parse XML-style tool calls from response
fn parse_tool_calls(response: &str) -> (String, Vec<ParsedToolCall>) {
    let mut text_parts = Vec::new();
    let mut calls = Vec::new();
    let mut remaining = response;

    while let Some(start) = remaining.find("<tool_call>") {
        let before = &remaining[..start];
        if !before.trim().is_empty() {
            text_parts.push(before.trim().to_string());
        }
        if let Some(end) = remaining[start..].find("</tool_call>") {
            let inner = &remaining[start + 11..start + end];
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(inner.trim()) {
                let name = parsed
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = parsed
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                calls.push(ParsedToolCall { name, arguments });
            }
            remaining = &remaining[start + end + 12..];
        } else {
            break;
        }
    }
    if !remaining.trim().is_empty() {
        text_parts.push(remaining.trim().to_string());
    }
    (text_parts.join("\n"), calls)
}

struct ParsedToolCall {
    name: String,
    arguments: serde_json::Value,
}

fn find_tool<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> Option<&'a dyn Tool> {
    tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
}

/// Agent turn that sends events to the TUI
async fn agent_turn_with_events(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &Arc<Vec<Box<dyn Tool>>>,
    observer: &dyn Observer,
    model: &str,
    temperature: f64,
    event_tx: &mpsc::Sender<AgentEvent>,
) -> Result<String> {
    for _iteration in 0..MAX_TOOL_ITERATIONS {
        // Use streaming if available for real-time token display
        let response = if provider.supports_streaming() {
            let (stream_tx, mut stream_rx) = mpsc::channel::<String>(64);
            let event_tx2 = event_tx.clone();

            // Forward stream tokens to TUI events
            let forwarder = tokio::spawn(async move {
                while let Some(token) = stream_rx.recv().await {
                    let _ = event_tx2.send(AgentEvent::Token(token)).await;
                }
            });

            let result = provider
                .chat_with_history_stream(history, model, temperature, stream_tx)
                .await;
            forwarder.abort();
            result?
        } else {
            let resp = provider
                .chat_with_history(history, model, temperature)
                .await?;
            let _ = event_tx.send(AgentEvent::Token(resp.clone())).await;
            resp
        };

        let (text, tool_calls) = parse_tool_calls(&response);

        if tool_calls.is_empty() {
            history.push(ChatMessage::assistant(&response));
            return Ok(if text.is_empty() { response } else { text });
        }

        // Notify TUI of all tool starts
        for call in &tool_calls {
            let _ = event_tx
                .send(AgentEvent::ToolStart(call.name.clone()))
                .await;
        }

        // Execute tools concurrently when multiple are requested
        let mut handles = Vec::with_capacity(tool_calls.len());
        for call in &tool_calls {
            let name = call.name.clone();
            let args = call.arguments.clone();
            let tools = Arc::clone(tools_registry);
            let tx = event_tx.clone();
            handles.push(tokio::spawn(async move {
                let start = Instant::now();
                let output = if let Some(tool) = tools.iter().find(|t| t.name() == name) {
                    match tool.execute(args).await {
                        Ok(r) if r.success => r.output,
                        Ok(r) => format!("Error: {}", r.error.unwrap_or_else(|| r.output)),
                        Err(e) => format!("Error executing {name}: {e}"),
                    }
                } else {
                    format!("Unknown tool: {name}")
                };
                let preview = if output.len() > 120 {
                    format!("{}...", &output[..120])
                } else {
                    output.clone()
                };
                let _ = tx
                    .send(AgentEvent::ToolResult {
                        name: name.clone(),
                        preview,
                    })
                    .await;
                (name, output, start.elapsed())
            }));
        }

        // Collect results in order
        let mut tool_results = String::new();
        for handle in handles {
            match handle.await {
                Ok((name, output, duration)) => {
                    observer.record_event(&ObserverEvent::ToolCall {
                        tool: name.clone(),
                        duration,
                        success: !output.starts_with("Error"),
                    });
                    let _ = writeln!(
                        tool_results,
                        "<tool_result name=\"{name}\">\n{output}\n</tool_result>",
                    );
                }
                Err(e) => {
                    let _ = writeln!(
                        tool_results,
                        "<tool_result name=\"unknown\">\nTask panicked: {e}\n</tool_result>",
                    );
                }
            }
        }

        history.push(ChatMessage::assistant(&response));
        history.push(ChatMessage::user(format!(
            "[Tool results]\n{tool_results}"
        )));
    }

    anyhow::bail!("Agent exceeded maximum tool iterations ({MAX_TOOL_ITERATIONS})")
}

fn build_tool_instructions(tools_registry: &[Box<dyn Tool>]) -> String {
    let mut instructions = String::new();
    instructions.push_str("\n## Tool Use Protocol\n\n");
    instructions.push_str(
        "To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n",
    );
    instructions.push_str(
        "```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n",
    );
    instructions.push_str("You may use multiple tool calls in a single response. ");
    instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
    instructions
        .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
    instructions.push_str("### Available Tools\n\n");

    for tool in tools_registry {
        let _ = writeln!(
            instructions,
            "**{}**: {}\nParameters: `{}`\n",
            tool.name(),
            tool.description(),
            tool.parameters_schema()
        );
    }

    instructions
}

fn trim_history(history: &mut Vec<ChatMessage>) {
    let has_system = history.first().map_or(false, |m| m.role == "system");
    let non_system_count = if has_system {
        history.len() - 1
    } else {
        history.len()
    };
    if non_system_count <= MAX_HISTORY_MESSAGES {
        return;
    }
    let start = if has_system { 1 } else { 0 };
    let to_remove = non_system_count - MAX_HISTORY_MESSAGES;
    history.drain(start..start + to_remove);
}
