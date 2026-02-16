use crate::providers::ChatMessage;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Metadata for a saved session (shown in listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub preview: String,
}

/// A full saved session.
#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

fn sessions_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("sessions")
}

fn session_path(workspace_dir: &Path, id: &str) -> PathBuf {
    sessions_dir(workspace_dir).join(format!("{id}.json"))
}

/// Generate a short session ID from timestamp.
pub fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ts:x}")
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Simple ISO-ish timestamp without chrono dependency
    format!("{secs}")
}

/// Save a session to disk.
pub fn save(
    workspace_dir: &Path,
    id: &str,
    model: &str,
    messages: &[ChatMessage],
) -> Result<PathBuf> {
    let dir = sessions_dir(workspace_dir);
    std::fs::create_dir_all(&dir)?;

    let now = now_iso();
    let session = Session {
        id: id.to_string(),
        created_at: now.clone(),
        updated_at: now,
        model: model.to_string(),
        messages: messages.to_vec(),
    };

    let path = session_path(workspace_dir, id);
    let json = serde_json::to_string_pretty(&session)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Update an existing session (preserves created_at).
pub fn update(
    workspace_dir: &Path,
    id: &str,
    model: &str,
    messages: &[ChatMessage],
) -> Result<PathBuf> {
    let path = session_path(workspace_dir, id);
    let created_at = if path.exists() {
        let existing: Session = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        existing.created_at
    } else {
        now_iso()
    };

    let session = Session {
        id: id.to_string(),
        created_at,
        updated_at: now_iso(),
        model: model.to_string(),
        messages: messages.to_vec(),
    };

    let dir = sessions_dir(workspace_dir);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(&session)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Load a session from disk.
pub fn load(workspace_dir: &Path, id: &str) -> Result<Session> {
    let path = session_path(workspace_dir, id);
    let json = std::fs::read_to_string(&path)?;
    let session: Session = serde_json::from_str(&json)?;
    Ok(session)
}

/// List all saved sessions (most recent first).
pub fn list(workspace_dir: &Path) -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir(workspace_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }
        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(session) = serde_json::from_str::<Session>(&json) {
                let preview = session
                    .messages
                    .iter()
                    .find(|m| m.role == "user")
                    .map(|m| {
                        if m.content.len() > 60 {
                            format!("{}...", &m.content[..60])
                        } else {
                            m.content.clone()
                        }
                    })
                    .unwrap_or_default();
                sessions.push(SessionMeta {
                    id: session.id,
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                    message_count: session.messages.len(),
                    preview,
                });
            }
        }
    }

    // Sort by updated_at descending
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

/// Delete a session.
pub fn delete(workspace_dir: &Path, id: &str) -> Result<()> {
    let path = session_path(workspace_dir, id);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_messages() -> Vec<ChatMessage> {
        vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ]
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        save(ws, "test-1", "gpt-4", &test_messages()).unwrap();
        let session = load(ws, "test-1").unwrap();

        assert_eq!(session.id, "test-1");
        assert_eq!(session.model, "gpt-4");
        assert_eq!(session.messages.len(), 3);
        assert_eq!(session.messages[0].role, "system");
        assert_eq!(session.messages[1].content, "Hello");
    }

    #[test]
    fn update_preserves_created_at() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        save(ws, "test-2", "gpt-4", &test_messages()).unwrap();
        let original = load(ws, "test-2").unwrap();

        let mut msgs = test_messages();
        msgs.push(ChatMessage::user("Follow up"));
        update(ws, "test-2", "gpt-4", &msgs).unwrap();

        let updated = load(ws, "test-2").unwrap();
        assert_eq!(updated.created_at, original.created_at);
        assert_eq!(updated.messages.len(), 4);
    }

    #[test]
    fn list_returns_sessions() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        save(ws, "a", "gpt-4", &test_messages()).unwrap();
        save(ws, "b", "gpt-4", &test_messages()).unwrap();

        let sessions = list(ws).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn list_empty_dir() {
        let dir = TempDir::new().unwrap();
        let sessions = list(dir.path()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn delete_removes_session() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        save(ws, "del-me", "gpt-4", &test_messages()).unwrap();
        assert!(load(ws, "del-me").is_ok());

        delete(ws, "del-me").unwrap();
        assert!(load(ws, "del-me").is_err());
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let dir = TempDir::new().unwrap();
        assert!(delete(dir.path(), "nonexistent").is_ok());
    }

    #[test]
    fn new_session_id_is_unique() {
        let a = new_session_id();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = new_session_id();
        assert_ne!(a, b);
    }

    #[test]
    fn preview_truncates_long_messages() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path();

        let msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user(&"x".repeat(200)),
        ];
        save(ws, "long", "gpt-4", &msgs).unwrap();

        let sessions = list(ws).unwrap();
        assert!(sessions[0].preview.len() <= 63); // 60 + "..."
    }
}
