#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMemory {
    pub id: i64,
    pub content: String,
    pub tags: Option<String>,
    pub source: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryCounts {
    pub memories: u64,
    pub messages: u64,
    pub context_items: u64,
    pub tool_invocations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocationRecord {
    pub session_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub source: String,
    pub status: String,
    pub risk_level: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAuditEntry {
    pub id: i64,
    pub tool_name: String,
    pub arguments_json: String,
    pub source: String,
    pub result_json: Option<String>,
    pub status: String,
    pub risk_level: String,
    pub requires_confirmation: bool,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}
