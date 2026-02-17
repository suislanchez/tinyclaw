use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

/// HTTP GET tool that fetches a URL and returns the body as text
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL via HTTP GET and return the response body as text"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("URL must start with http:// or https://".into()),
            });
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();

                // Truncate large responses
                let max_len = 50_000;
                let (body, truncated) = if body.len() > max_len {
                    (body[..max_len].to_string(), true)
                } else {
                    (body, false)
                };

                let suffix = if truncated {
                    "\n... [truncated]"
                } else {
                    ""
                };

                if status.is_success() {
                    Ok(ToolResult {
                        success: true,
                        output: format!("HTTP {status}\n{body}{suffix}"),
                        error: None,
                    })
                } else {
                    Ok(ToolResult {
                        success: false,
                        output: format!("HTTP {status}\n{body}{suffix}"),
                        error: Some(format!("HTTP {status}")),
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Request failed: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_fetch_name() {
        let tool = WebFetchTool::new();
        assert_eq!(tool.name(), "web_fetch");
    }

    #[test]
    fn web_fetch_schema() {
        let tool = WebFetchTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["url"].is_object());
    }

    #[tokio::test]
    async fn web_fetch_rejects_non_http() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({"url": "ftp://example.com"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap().contains("http"));
    }

    #[tokio::test]
    async fn web_fetch_missing_url() {
        let tool = WebFetchTool::new();
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }
}
