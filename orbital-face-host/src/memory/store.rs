use super::migrations;
use super::models::{MemoryCounts, ToolAuditEntry, ToolInvocationRecord, UserMemory};
use crate::context::ContextItem;
use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub store_session_messages: bool,
    pub store_context_items: bool,
    pub db_path: Option<PathBuf>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            store_session_messages: false,
            store_context_items: false,
            db_path: None,
        }
    }
}

pub trait MemoryStore {
    fn path(&self) -> &Path;
    fn start_session(&self, provider: &str, model: &str) -> anyhow::Result<String>;
    fn end_session(&self, session_id: &str) -> anyhow::Result<()>;
    fn add_message(&self, session_id: &str, role: &str, content: &str) -> anyhow::Result<()>;
    fn add_context_item(&self, session_id: &str, item: &ContextItem) -> anyhow::Result<()>;
    fn remember(&self, content: &str, source: &str) -> anyhow::Result<UserMemory>;
    fn list_memories(&self, limit: usize) -> anyhow::Result<Vec<UserMemory>>;
    fn search_memories(&self, query: &str, limit: usize) -> anyhow::Result<Vec<UserMemory>>;
    fn forget_memory(&self, id: i64) -> anyhow::Result<bool>;
    fn counts(&self, session_id: &str) -> anyhow::Result<MemoryCounts>;
    fn clear_session(&self, session_id: &str) -> anyhow::Result<()>;
    fn begin_tool_invocation(&self, record: &ToolInvocationRecord) -> anyhow::Result<i64>;
    fn update_tool_invocation(
        &self,
        id: i64,
        status: &str,
        result_json: Option<&str>,
        error: Option<&str>,
    ) -> anyhow::Result<()>;
    fn recent_tool_invocations(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ToolAuditEntry>>;
    fn clear_tool_session(&self, session_id: &str) -> anyhow::Result<usize>;
}

pub struct SqliteMemoryStore {
    path: PathBuf,
    connection: Connection,
}

impl SqliteMemoryStore {
    pub fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        if path != Path::new(":memory:") {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create memory directory {}", parent.display())
                })?;
            }
        }
        let connection = Connection::open(&path)
            .with_context(|| format!("failed to open memory database {}", path.display()))?;
        migrations::migrate(&connection).context("failed to migrate memory database")?;
        Ok(Self { path, connection })
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn path(&self) -> &Path {
        &self.path
    }

    fn start_session(&self, provider: &str, model: &str) -> anyhow::Result<String> {
        let id = format!("session-{}-{}", std::process::id(), unique_timestamp());
        self.connection.execute(
            "INSERT INTO sessions(id, started_at, model_provider, model_name) VALUES(?1, unixepoch(), ?2, ?3)",
            params![id, provider, model],
        )?;
        Ok(id)
    }

    fn end_session(&self, session_id: &str) -> anyhow::Result<()> {
        self.connection.execute(
            "UPDATE sessions SET ended_at=unixepoch() WHERE id=?1",
            [session_id],
        )?;
        Ok(())
    }

    fn add_message(&self, session_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(role, "user" | "assistant" | "system"),
            "invalid message role"
        );
        self.connection.execute(
            "INSERT INTO messages(session_id, role, content, created_at) VALUES(?1, ?2, ?3, unixepoch())",
            params![session_id, role, content],
        )?;
        Ok(())
    }

    fn add_context_item(&self, session_id: &str, item: &ContextItem) -> anyhow::Result<()> {
        self.connection.execute(
            "INSERT INTO context_items(session_id, kind, title, source, content, size_chars, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![session_id, item.kind.as_str(), item.title, item.source, item.content, item.size_chars as i64, item.created_at as i64],
        )?;
        Ok(())
    }

    fn remember(&self, content: &str, source: &str) -> anyhow::Result<UserMemory> {
        anyhow::ensure!(
            !content.trim().is_empty(),
            "memory content must not be empty"
        );
        self.connection.execute(
            "INSERT INTO user_memories(content, source, created_at, updated_at) VALUES(?1, ?2, unixepoch(), unixepoch())",
            params![content.trim(), source],
        )?;
        let id = self.connection.last_insert_rowid();
        self.memory_by_id(id)?
            .context("inserted memory was not found")
    }

    fn list_memories(&self, limit: usize) -> anyhow::Result<Vec<UserMemory>> {
        self.query_memories(
            "SELECT id, content, tags, source, created_at, updated_at FROM user_memories WHERE deleted_at IS NULL ORDER BY id DESC LIMIT ?1",
            params![bounded_limit(limit)],
        )
    }

    fn search_memories(&self, query: &str, limit: usize) -> anyhow::Result<Vec<UserMemory>> {
        anyhow::ensure!(
            !query.trim().is_empty(),
            "memory search query must not be empty"
        );
        self.query_memories(
            "SELECT id, content, tags, source, created_at, updated_at FROM user_memories WHERE deleted_at IS NULL AND content LIKE ?1 ESCAPE '\\' COLLATE NOCASE ORDER BY id DESC LIMIT ?2",
            params![format!("%{}%", escape_like(query.trim())), bounded_limit(limit)],
        )
    }

    fn forget_memory(&self, id: i64) -> anyhow::Result<bool> {
        Ok(self.connection.execute(
            "UPDATE user_memories SET deleted_at=unixepoch(), updated_at=unixepoch() WHERE id=?1 AND deleted_at IS NULL",
            [id],
        )? == 1)
    }

    fn counts(&self, session_id: &str) -> anyhow::Result<MemoryCounts> {
        Ok(MemoryCounts {
            memories: self.count(
                "SELECT COUNT(*) FROM user_memories WHERE deleted_at IS NULL",
                [],
            )?,
            messages: self.count(
                "SELECT COUNT(*) FROM messages WHERE session_id=?1",
                [session_id],
            )?,
            context_items: self.count(
                "SELECT COUNT(*) FROM context_items WHERE session_id=?1",
                [session_id],
            )?,
            tool_invocations: self.count(
                "SELECT COUNT(*) FROM tool_invocations WHERE session_id=?1",
                [session_id],
            )?,
        })
    }

    fn clear_session(&self, session_id: &str) -> anyhow::Result<()> {
        self.connection
            .execute("DELETE FROM messages WHERE session_id=?1", [session_id])?;
        self.connection.execute(
            "DELETE FROM context_items WHERE session_id=?1",
            [session_id],
        )?;
        self.connection.execute(
            "DELETE FROM tool_invocations WHERE session_id=?1",
            [session_id],
        )?;
        Ok(())
    }

    fn begin_tool_invocation(&self, record: &ToolInvocationRecord) -> anyhow::Result<i64> {
        self.connection.execute(
            "INSERT INTO tool_invocations(session_id, tool_name, arguments_json, source, status, risk_level, requires_confirmation, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, unixepoch())",
            params![record.session_id, record.tool_name, record.arguments_json, record.source, record.status, record.risk_level, record.requires_confirmation],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    fn update_tool_invocation(
        &self,
        id: i64,
        status: &str,
        result_json: Option<&str>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        self.connection.execute(
            "UPDATE tool_invocations SET status=?2, result_json=?3, error=?4, completed_at=CASE WHEN ?2 IN ('denied','executed','failed') THEN unixepoch() ELSE completed_at END WHERE id=?1",
            params![id, status, result_json, error],
        )?;
        Ok(())
    }

    fn recent_tool_invocations(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ToolAuditEntry>> {
        let mut statement = self.connection.prepare(
            "SELECT id, tool_name, arguments_json, source, result_json, status, risk_level, requires_confirmation, created_at, completed_at, error FROM tool_invocations WHERE session_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![session_id, bounded_limit(limit)], |row| {
            Ok(ToolAuditEntry {
                id: row.get(0)?,
                tool_name: row.get(1)?,
                arguments_json: row.get(2)?,
                source: row.get(3)?,
                result_json: row.get(4)?,
                status: row.get(5)?,
                risk_level: row.get(6)?,
                requires_confirmation: row.get(7)?,
                created_at: row.get(8)?,
                completed_at: row.get(9)?,
                error: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn clear_tool_session(&self, session_id: &str) -> anyhow::Result<usize> {
        Ok(self.connection.execute(
            "DELETE FROM tool_invocations WHERE session_id=?1",
            [session_id],
        )?)
    }
}

impl SqliteMemoryStore {
    fn memory_by_id(&self, id: i64) -> anyhow::Result<Option<UserMemory>> {
        Ok(self.connection.query_row(
            "SELECT id, content, tags, source, created_at, updated_at FROM user_memories WHERE id=?1 AND deleted_at IS NULL",
            [id], map_memory,
        ).optional()?)
    }

    fn query_memories<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> anyhow::Result<Vec<UserMemory>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, map_memory)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn count<P: rusqlite::Params>(&self, sql: &str, params: P) -> anyhow::Result<u64> {
        Ok(self.connection.query_row(sql, params, |row| row.get(0))?)
    }
}

fn map_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserMemory> {
    Ok(UserMemory {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: row.get(2)?,
        source: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn bounded_limit(limit: usize) -> i64 {
    limit.clamp(1, 100) as i64
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn unique_timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

pub fn default_database_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ORBITAL_DB_PATH") {
        return path.into();
    }
    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return PathBuf::from(app_data).join("Orbital").join("orbital.db");
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("orbital").join("orbital.db");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".local/share/orbital/orbital.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SqliteMemoryStore {
        SqliteMemoryStore::open(":memory:").unwrap()
    }

    #[test]
    fn migrations_initialize_all_tables() {
        let store = store();
        for table in [
            "sessions",
            "messages",
            "context_items",
            "user_memories",
            "tool_invocations",
            "settings",
        ] {
            let exists: bool = store
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing {table}");
        }
    }

    #[test]
    fn file_store_initializes_and_reopens() {
        let path = std::env::temp_dir().join(format!(
            "orbital-memory-test-{}-{}.db",
            std::process::id(),
            unique_timestamp()
        ));
        {
            let store = SqliteMemoryStore::open(&path).unwrap();
            store.remember("persistent test", "test").unwrap();
        }
        let reopened = SqliteMemoryStore::open(&path).unwrap();
        assert_eq!(reopened.list_memories(10).unwrap().len(), 1);
        drop(reopened);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }

    #[test]
    fn remembers_searches_and_soft_deletes() {
        let store = store();
        let memory = store
            .remember("Orbital is local-first", "terminal")
            .unwrap();
        assert_eq!(store.list_memories(10).unwrap().len(), 1);
        assert_eq!(store.search_memories("LOCAL", 10).unwrap()[0].id, memory.id);
        assert!(store.forget_memory(memory.id).unwrap());
        assert!(store.list_memories(10).unwrap().is_empty());
        assert!(!store.forget_memory(memory.id).unwrap());
    }

    #[test]
    fn session_logging_is_available_but_default_config_disables_it() {
        assert!(!MemoryConfig::default().store_session_messages);
        let store = store();
        let session = store.start_session("mock", "test").unwrap();
        store.add_message(&session, "user", "hello").unwrap();
        assert_eq!(store.counts(&session).unwrap().messages, 1);
        store.clear_session(&session).unwrap();
        assert_eq!(store.counts(&session).unwrap().messages, 0);
    }

    #[test]
    fn tool_audit_tracks_denied_and_successful_invocations() {
        let store = store();
        let session = store.start_session("mock", "test").unwrap();
        let record = ToolInvocationRecord {
            session_id: session.clone(),
            tool_name: "file.read_text".into(),
            arguments_json: "{\"path\":\"README.md\"}".into(),
            source: "manual".into(),
            status: "proposed".into(),
            risk_level: "medium".into(),
            requires_confirmation: true,
        };
        let denied = store.begin_tool_invocation(&record).unwrap();
        store
            .update_tool_invocation(denied, "denied", None, None)
            .unwrap();
        let mut approved = record;
        approved.tool_name = "time.now".into();
        approved.status = "approved".into();
        approved.risk_level = "low".into();
        approved.requires_confirmation = false;
        let executed = store.begin_tool_invocation(&approved).unwrap();
        store
            .update_tool_invocation(executed, "executed", Some("{}"), None)
            .unwrap();
        let history = store.recent_tool_invocations(&session, 10).unwrap();
        assert_eq!(history[0].status, "executed");
        assert_eq!(history[1].status, "denied");
    }
}
