use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

/// Regex search across workspace files
pub struct SearchFilesTool {
    security: Arc<SecurityPolicy>,
}

impl SearchFilesTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern across files in the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Subdirectory to search in (default: entire workspace)"
                },
                "glob": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs' or '*.py'"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'pattern' parameter"))?;

        let subdir = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let glob_filter = args.get("glob").and_then(|v| v.as_str());

        if !self.security.is_path_allowed(subdir) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {subdir}")),
            });
        }

        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Invalid regex: {e}")),
                });
            }
        };

        let search_dir = self.security.workspace_dir.join(subdir);
        let resolved = match tokio::fs::canonicalize(&search_dir).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot resolve search path: {e}")),
                });
            }
        };

        if !self.security.is_resolved_path_allowed(&resolved) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Search path escapes workspace".into()),
            });
        }

        let glob_pat = glob_filter.map(|g| glob::Pattern::new(g).ok()).flatten();

        let mut results = String::new();
        let mut match_count: usize = 0;
        const MAX_MATCHES: usize = 100;

        search_recursive(&resolved, &re, &glob_pat, &mut results, &mut match_count, MAX_MATCHES).await;

        if match_count == 0 {
            return Ok(ToolResult {
                success: true,
                output: "No matches found.".into(),
                error: None,
            });
        }

        let truncated = if match_count >= MAX_MATCHES {
            format!("\n... truncated at {MAX_MATCHES} matches")
        } else {
            String::new()
        };

        Ok(ToolResult {
            success: true,
            output: format!("{match_count} matches:{truncated}\n{results}"),
            error: None,
        })
    }
}

async fn search_recursive(
    dir: &std::path::Path,
    re: &regex::Regex,
    glob_pat: &Option<glob::Pattern>,
    results: &mut String,
    match_count: &mut usize,
    max: usize,
) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        if *match_count >= max {
            return;
        }

        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden files/dirs and common non-text dirs
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }

        if let Ok(ft) = entry.file_type().await {
            if ft.is_dir() {
                Box::pin(search_recursive(&path, re, glob_pat, results, match_count, max)).await;
            } else if ft.is_file() {
                if let Some(ref pat) = glob_pat {
                    if !pat.matches(&name) {
                        continue;
                    }
                }

                // Skip binary/large files
                if let Ok(meta) = entry.metadata().await {
                    if meta.len() > 1_000_000 {
                        continue;
                    }
                }

                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    let rel = path.strip_prefix(dir).unwrap_or(&path);
                    for (line_num, line) in content.lines().enumerate() {
                        if *match_count >= max {
                            return;
                        }
                        if re.is_match(line) {
                            let _ = writeln!(results, "{}:{}: {}", rel.display(), line_num + 1, line);
                            *match_count += 1;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{AutonomyLevel, SecurityPolicy};

    fn test_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: workspace,
            ..SecurityPolicy::default()
        })
    }

    #[test]
    fn search_files_name() {
        let tool = SearchFilesTool::new(test_security(std::env::temp_dir()));
        assert_eq!(tool.name(), "search_files");
    }

    #[test]
    fn search_files_schema() {
        let tool = SearchFilesTool::new(test_security(std::env::temp_dir()));
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["pattern"].is_object());
    }

    #[tokio::test]
    async fn search_files_finds_match() {
        let dir = std::env::temp_dir().join("tinyclaw_test_search");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("hello.txt"), "foo bar\nbaz qux\nfoo again").await.unwrap();

        let tool = SearchFilesTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"pattern": "foo"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("2 matches"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn search_files_no_matches() {
        let dir = std::env::temp_dir().join("tinyclaw_test_search_none");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("hello.txt"), "nothing here").await.unwrap();

        let tool = SearchFilesTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"pattern": "xyz123"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No matches"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn search_files_invalid_regex() {
        let dir = std::env::temp_dir().join("tinyclaw_test_search_invalid");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = SearchFilesTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"pattern": "[invalid"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("Invalid regex"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn search_files_with_glob() {
        let dir = std::env::temp_dir().join("tinyclaw_test_search_glob");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("code.rs"), "fn main() {}").await.unwrap();
        tokio::fs::write(dir.join("readme.md"), "fn not_code").await.unwrap();

        let tool = SearchFilesTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"pattern": "fn", "glob": "*.rs"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("1 match"));
        assert!(result.output.contains("code.rs"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
