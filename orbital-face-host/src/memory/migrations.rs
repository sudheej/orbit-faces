use rusqlite::Connection;

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    profile TEXT,
    model_provider TEXT NOT NULL,
    model_name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK(role IN ('user','assistant','system')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS context_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    content TEXT NOT NULL,
    size_chars INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS user_memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    tags TEXT,
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);
CREATE TABLE IF NOT EXISTS tool_invocations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('manual','model_suggested')),
    result_json TEXT,
    status TEXT NOT NULL CHECK(status IN ('proposed','approved','denied','executed','failed')),
    risk_level TEXT NOT NULL CHECK(risk_level IN ('low','medium','high')),
    requires_confirmation INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    error TEXT
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);
CREATE INDEX IF NOT EXISTS idx_context_session ON context_items(session_id, id);
CREATE INDEX IF NOT EXISTS idx_memories_active ON user_memories(deleted_at, id DESC);
CREATE INDEX IF NOT EXISTS idx_tools_session ON tool_invocations(session_id, id DESC);
"#;

pub fn migrate(connection: &Connection) -> anyhow::Result<()> {
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);",
    )?;
    let applied = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=1)",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !applied {
        connection.execute_batch(MIGRATION_V1)?;
        connection.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES(1, unixepoch())",
            [],
        )?;
    }
    let has_source = {
        let mut statement = connection.prepare("PRAGMA table_info(tool_invocations)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        columns
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|column| column == "source")
    };
    if !has_source {
        connection.execute_batch(
            "ALTER TABLE tool_invocations ADD COLUMN source TEXT NOT NULL DEFAULT 'manual' CHECK(source IN ('manual','model_suggested'));",
        )?;
    }
    connection.execute(
        "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES(2, unixepoch())",
        [],
    )?;
    Ok(())
}
