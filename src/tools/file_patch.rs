use super::traits::{Tool, ToolResult};
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Targeted file editing via old_string/new_string replacement
pub struct FilePatchTool {
    security: Arc<SecurityPolicy>,
}

impl FilePatchTool {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for FilePatchTool {
    fn name(&self) -> &str {
        "file_patch"
    }

    fn description(&self) -> &str {
        "Apply a targeted edit to a file by replacing an exact string match"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace"
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to find and replace (must match exactly once)"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'old_string' parameter"))?;

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'new_string' parameter"))?;

        if !self.security.is_path_allowed(path) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Path not allowed by security policy: {path}")),
            });
        }

        let full_path = self.security.workspace_dir.join(path);

        // Resolve to block symlink escapes
        let resolved = match tokio::fs::canonicalize(&full_path).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Cannot resolve path: {e}")),
                });
            }
        };

        if !self.security.is_resolved_path_allowed(&resolved) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Resolved path escapes workspace: {}",
                    resolved.display()
                )),
            });
        }

        let content = match tokio::fs::read_to_string(&resolved).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),
                });
            }
        };

        let count = content.matches(old_string).count();
        if count == 0 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("old_string not found in file".into()),
            });
        }
        if count > 1 {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "old_string found {count} times — must match exactly once. Provide more context."
                )),
            });
        }

        let new_content = content.replacen(old_string, new_string, 1);

        match tokio::fs::write(&resolved, &new_content).await {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: format!("Patched {path} ({} bytes)", new_content.len()),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write file: {e}")),
            }),
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
    fn file_patch_name() {
        let tool = FilePatchTool::new(test_security(std::env::temp_dir()));
        assert_eq!(tool.name(), "file_patch");
    }

    #[test]
    fn file_patch_schema() {
        let tool = FilePatchTool::new(test_security(std::env::temp_dir()));
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["old_string"].is_object());
        assert!(schema["properties"]["new_string"].is_object());
    }

    #[tokio::test]
    async fn file_patch_replaces_exact_match() {
        let dir = std::env::temp_dir().join("tinyclaw_test_file_patch");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world").await.unwrap();

        let tool = FilePatchTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "test.txt", "old_string": "hello", "new_string": "goodbye"}))
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.error);

        let content = tokio::fs::read_to_string(dir.join("test.txt")).await.unwrap();
        assert_eq!(content, "goodbye world");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_patch_fails_when_not_found() {
        let dir = std::env::temp_dir().join("tinyclaw_test_file_patch_nf");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world").await.unwrap();

        let tool = FilePatchTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "test.txt", "old_string": "xyz", "new_string": "abc"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("not found"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_patch_fails_when_ambiguous() {
        let dir = std::env::temp_dir().join("tinyclaw_test_file_patch_ambig");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "aaa bbb aaa").await.unwrap();

        let tool = FilePatchTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "test.txt", "old_string": "aaa", "new_string": "ccc"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("2 times"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_patch_blocks_path_traversal() {
        let dir = std::env::temp_dir().join("tinyclaw_test_file_patch_traversal");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FilePatchTool::new(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "../../etc/passwd", "old_string": "a", "new_string": "b"}))
            .await
            .unwrap();
        assert!(!result.success);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
