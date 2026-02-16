#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    dead_code
)]

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// ── Always compiled (tiny tier) ────────────────────────────────
mod agent;
mod channels;
mod config;
mod health;
mod identity;
mod memory;
mod observability;
mod onboard;
mod providers;
mod runtime;
mod security;
mod session;
mod skills;
mod tools;
mod util;

// ── Standard tier (+TUI) ──────────────────────────────────────
#[cfg(feature = "tui-feature")]
mod tui;

// ── Full tier (+gateway, daemon, channels, scheduler, etc.) ───
#[cfg(feature = "gateway-feature")]
mod gateway;
#[cfg(feature = "daemon-feature")]
mod daemon;
#[cfg(feature = "daemon-feature")]
mod cron;
#[cfg(feature = "daemon-feature")]
mod doctor;
#[cfg(feature = "daemon-feature")]
mod heartbeat;
#[cfg(feature = "daemon-feature")]
mod service;
#[cfg(feature = "full")]
mod integrations;
#[cfg(feature = "full")]
mod migration;
#[cfg(feature = "skillforge-feature")]
mod skillforge;
#[cfg(feature = "tunnel-feature")]
mod tunnel;

use config::Config;

/// `TinyClaw` - Zero overhead. Zero compromise. 100% Rust.
#[derive(Parser, Debug)]
#[command(name = "tinyclaw")]
#[command(version = "0.1.0")]
#[command(about = "Ultra-efficient AI assistant. Fork of ZeroClaw.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[cfg(feature = "daemon-feature")]
#[derive(Subcommand, Debug)]
enum ServiceCommands {
    /// Install daemon service unit for auto-start and restart
    Install,
    /// Start daemon service
    Start,
    /// Stop daemon service
    Stop,
    /// Check daemon service status
    Status,
    /// Uninstall daemon service unit
    Uninstall,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize your workspace and configuration
    Onboard {
        /// Run the full interactive wizard (default is quick setup)
        #[arg(long)]
        interactive: bool,

        /// Reconfigure channels only (fast repair flow)
        #[arg(long)]
        channels_only: bool,

        /// API key (used in quick mode, ignored with --interactive)
        #[arg(long)]
        api_key: Option<String>,

        /// Provider name (used in quick mode, default: openrouter)
        #[arg(long)]
        provider: Option<String>,

        /// Memory backend (sqlite, markdown, none) - used in quick mode, default: sqlite
        #[arg(long)]
        memory: Option<String>,
    },

    /// Launch the TUI (terminal UI) chat interface
    #[cfg(feature = "tui-feature")]
    Tui {
        /// Provider to use (openrouter, anthropic, openai, ollama)
        #[arg(short, long)]
        provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0)
        #[arg(short, long, default_value = "0.7")]
        temperature: f64,
    },

    /// Start the AI agent loop
    Agent {
        /// Single message mode (don't enter interactive mode)
        #[arg(short, long)]
        message: Option<String>,

        /// Provider to use (openrouter, anthropic, openai)
        #[arg(short, long)]
        provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0)
        #[arg(short, long, default_value = "0.7")]
        temperature: f64,
    },

    /// Start the gateway server (webhooks, websockets)
    #[cfg(feature = "gateway-feature")]
    Gateway {
        /// Port to listen on (use 0 for random available port)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Start long-running autonomous runtime (gateway + channels + heartbeat + scheduler)
    #[cfg(feature = "daemon-feature")]
    Daemon {
        /// Port to listen on (use 0 for random available port)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Manage OS service lifecycle (launchd/systemd user service)
    #[cfg(feature = "daemon-feature")]
    Service {
        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// Run diagnostics for daemon/scheduler/channel freshness
    #[cfg(feature = "daemon-feature")]
    Doctor,

    /// Show system status (full details)
    Status,

    /// Configure and manage scheduled tasks
    #[cfg(feature = "daemon-feature")]
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// Manage channels (telegram, discord, slack)
    #[cfg(feature = "channels-feature")]
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// Browse 50+ integrations
    #[cfg(feature = "full")]
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// Manage skills (user-defined capabilities)
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },

    /// Migrate data from other agent runtimes
    #[cfg(feature = "full")]
    Migrate {
        #[command(subcommand)]
        migrate_command: MigrateCommands,
    },
}

#[cfg(feature = "full")]
#[derive(Subcommand, Debug)]
enum MigrateCommands {
    /// Import memory from an `OpenClaw` workspace into this `TinyClaw` workspace
    Openclaw {
        /// Optional path to `OpenClaw` workspace (defaults to ~/.openclaw/workspace)
        #[arg(long)]
        source: Option<std::path::PathBuf>,

        /// Validate and preview migration without writing any data
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(feature = "daemon-feature")]
#[derive(Subcommand, Debug)]
enum CronCommands {
    /// List all scheduled tasks
    List,
    /// Add a new scheduled task
    Add {
        /// Cron expression
        expression: String,
        /// Command to run
        command: String,
    },
    /// Remove a scheduled task
    Remove {
        /// Task ID
        id: String,
    },
}

#[cfg(feature = "channels-feature")]
#[derive(Subcommand, Debug)]
enum ChannelCommands {
    /// List configured channels
    List,
    /// Start all configured channels (Telegram, Discord, Slack)
    Start,
    /// Run health checks for configured channels
    Doctor,
    /// Add a new channel
    Add {
        /// Channel type
        channel_type: String,
        /// Configuration JSON
        config: String,
    },
    /// Remove a channel
    Remove {
        /// Channel name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum SkillCommands {
    /// List installed skills
    List,
    /// Install a skill from a GitHub URL or local path
    Install {
        /// GitHub URL or local path
        source: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name
        name: String,
    },
}

#[cfg(feature = "full")]
#[derive(Subcommand, Debug)]
enum IntegrationCommands {
    /// Show details about a specific integration
    Info {
        /// Integration name
        name: String,
    },
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    // Install default crypto provider for Rustls TLS.
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Warning: Failed to install default crypto provider: {e:?}");
    }

    let cli = Cli::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    // Onboard runs quick setup by default, or the interactive wizard with --interactive
    if let Commands::Onboard {
        interactive,
        channels_only,
        api_key,
        provider,
        memory,
    } = &cli.command
    {
        if *interactive && *channels_only {
            bail!("Use either --interactive or --channels-only, not both");
        }
        if *channels_only && (api_key.is_some() || provider.is_some() || memory.is_some()) {
            bail!("--channels-only does not accept --api-key, --provider, or --memory");
        }

        let config = if *channels_only {
            onboard::run_channels_repair_wizard()?
        } else if *interactive {
            onboard::run_wizard()?
        } else {
            onboard::run_quick_setup(api_key.as_deref(), provider.as_deref(), memory.as_deref())?
        };
        // Auto-start channels if user said yes during wizard
        #[cfg(feature = "channels-feature")]
        if std::env::var("ZEROCLAW_AUTOSTART_CHANNELS").as_deref() == Ok("1") {
            channels::start_channels(config).await?;
        }
        return Ok(());
    }

    // All other commands need config loaded first
    let config = Config::load_or_init()?;

    match cli.command {
        Commands::Onboard { .. } => unreachable!(),

        #[cfg(feature = "tui-feature")]
        Commands::Tui {
            provider,
            model,
            temperature,
        } => tui::run(config, provider, model, temperature).await,

        Commands::Agent {
            message,
            provider,
            model,
            temperature,
        } => agent::run(config, message, provider, model, temperature).await,

        #[cfg(feature = "gateway-feature")]
        Commands::Gateway { port, host } => {
            if port == 0 {
                info!("Starting TinyClaw Gateway on {host} (random port)");
            } else {
                info!("Starting TinyClaw Gateway on {host}:{port}");
            }
            gateway::run_gateway(&host, port, config).await
        }

        #[cfg(feature = "daemon-feature")]
        Commands::Daemon { port, host } => {
            if port == 0 {
                info!("Starting TinyClaw Daemon on {host} (random port)");
            } else {
                info!("Starting TinyClaw Daemon on {host}:{port}");
            }
            daemon::run(config, host, port).await
        }

        Commands::Status => {
            println!("TinyClaw Status");
            println!();
            println!("Version:     {}", env!("CARGO_PKG_VERSION"));
            println!("Workspace:   {}", config.workspace_dir.display());
            println!("Config:      {}", config.config_path.display());
            println!();
            println!(
                "Provider:      {}",
                config.default_provider.as_deref().unwrap_or("openrouter")
            );
            println!(
                "Model:         {}",
                config.default_model.as_deref().unwrap_or("(default)")
            );
            println!("Observability: {}", config.observability.backend);
            println!("Autonomy:      {:?}", config.autonomy.level);
            println!("Runtime:       {}", config.runtime.kind);
            println!(
                "Heartbeat:     {}",
                if config.heartbeat.enabled {
                    format!("every {}min", config.heartbeat.interval_minutes)
                } else {
                    "disabled".into()
                }
            );
            println!(
                "Memory:        {} (auto-save: {})",
                config.memory.backend,
                if config.memory.auto_save { "on" } else { "off" }
            );

            #[cfg(feature = "tiny")]
            {
                let tier = if cfg!(feature = "full") {
                    "full"
                } else if cfg!(feature = "standard") {
                    "standard"
                } else {
                    "tiny"
                };
                println!("Build tier:    {tier}");
            }

            Ok(())
        }

        #[cfg(feature = "daemon-feature")]
        Commands::Cron { cron_command } => cron::handle_command(cron_command, &config),

        #[cfg(feature = "daemon-feature")]
        Commands::Service { service_command } => service::handle_command(&service_command, &config),

        #[cfg(feature = "daemon-feature")]
        Commands::Doctor => doctor::run(&config),

        #[cfg(feature = "channels-feature")]
        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => channels::start_channels(config).await,
            ChannelCommands::Doctor => channels::doctor_channels(config).await,
            other => channels::handle_command(other, &config),
        },

        #[cfg(feature = "full")]
        Commands::Integrations {
            integration_command,
        } => integrations::handle_command(integration_command, &config),

        Commands::Skills { skill_command } => {
            skills::handle_command(skill_command, &config.workspace_dir)
        }

        #[cfg(feature = "full")]
        Commands::Migrate { migrate_command } => {
            migration::handle_command(migrate_command, &config).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }
}
