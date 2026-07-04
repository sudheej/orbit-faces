use crate::context::active_window::ActiveWindowProvider;
use crate::context::attachments;
use crate::context::clipboard::ClipboardProvider;
use crate::context::ContextManager;
use crate::memory::MemoryStore;
use anyhow::Context;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Manual,
    ModelSuggested,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema_json: Value,
    pub output_schema_json: Option<Value>,
    pub risk_level: RiskLevel,
    pub requires_confirmation: bool,
    pub local_only: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub arguments: Value,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermission {
    Execute,
    Confirm,
    Deny,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub result: Value,
    pub face_state: Option<(String, String)>,
}

pub struct ToolEnvironment<'a> {
    pub context: &'a ContextManager,
    pub memory: &'a dyn MemoryStore,
    pub active_window: &'a dyn ActiveWindowProvider,
    pub clipboard: &'a dyn ClipboardProvider,
}

#[derive(Debug, Default)]
pub struct ToolRegistry {
    definitions: BTreeMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for definition in builtin_definitions() {
            registry
                .register(definition)
                .expect("built-in tool names are unique");
        }
        registry
    }

    pub fn register(&mut self, definition: ToolDefinition) -> anyhow::Result<()> {
        anyhow::ensure!(
            !definition.name.trim().is_empty(),
            "tool name must not be empty"
        );
        anyhow::ensure!(
            definition.input_schema_json.is_object(),
            "tool input schema must be an object"
        );
        anyhow::ensure!(
            !self.definitions.contains_key(&definition.name),
            "duplicate tool name {:?}",
            definition.name
        );
        self.definitions.insert(definition.name.clone(), definition);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.get(name)
    }

    pub fn list(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.definitions.values()
    }

    pub fn validate(&self, name: &str, arguments: &Value) -> anyhow::Result<()> {
        let definition = self
            .get(name)
            .with_context(|| format!("unknown tool {name:?}"))?;
        validate_schema(arguments, &definition.input_schema_json)
    }

    pub fn prompt_catalog(&self) -> String {
        self.list()
            .map(|tool| {
                format!(
                    "- {}: {} schema={}",
                    tool.name, tool.description, tool.input_schema_json
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn capability_catalog(&self) -> String {
        self.list()
            .map(|tool| format!("- {}: {}", tool.name, tool.description))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn permission_for(definition: &ToolDefinition, _source: ToolSource) -> ToolPermission {
    if !definition.local_only {
        return ToolPermission::Deny;
    }
    if definition.risk_level == RiskLevel::High {
        ToolPermission::Confirm
    } else {
        ToolPermission::Execute
    }
}

pub fn execute_tool(
    invocation: &ToolInvocation,
    environment: &ToolEnvironment<'_>,
) -> anyhow::Result<ToolOutput> {
    let args = invocation
        .arguments
        .as_object()
        .context("tool arguments must be a JSON object")?;
    let result = match invocation.tool_name.as_str() {
        "time.now" => {
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            json!({
                "unix_timestamp": seconds,
                "timezone": std::env::var("TZ").unwrap_or_else(|_| "system-local".into())
            })
        }
        "context.list" => json!({"items": environment.context.items().iter().map(|item| json!({
            "id": item.id, "kind": item.kind.as_str(), "title": item.title,
            "source": item.source, "size_chars": item.size_chars
        })).collect::<Vec<_>>() }),
        "context.get" => {
            let id = string_arg(args, "id")?;
            let item = environment
                .context
                .items()
                .iter()
                .find(|item| item.id == id)
                .with_context(|| format!("context item {id:?} not found"))?;
            json!({"id": item.id, "kind": item.kind.as_str(), "title": item.title, "source": item.source, "content": item.content})
        }
        "active_window.get" => {
            let window = environment.active_window.get_active_window()?;
            json!({"title": window.title, "process_name": window.process_name, "process_id": window.process_id, "platform": window.platform})
        }
        "clipboard.read" => json!({"text": environment.clipboard.read_text()?}),
        "memory.remember" => {
            let content = string_arg(args, "content")?;
            if content.to_ascii_lowercase().starts_with("user's name is ") {
                for existing in environment.memory.search_memories("User's name is", 100)? {
                    environment.memory.forget_memory(existing.id)?;
                }
            }
            let memory = environment.memory.remember(content, "tool")?;
            json!({"id": memory.id, "content": memory.content})
        }
        "memory.search" => {
            let memories = environment
                .memory
                .search_memories(string_arg(args, "query")?, 20)?;
            json!({"memories": memories.iter().map(|memory| json!({"id": memory.id, "content": memory.content, "source": memory.source})).collect::<Vec<_>>()})
        }
        "memory.list" => {
            let memories = environment.memory.list_memories(20)?;
            json!({"memories": memories.iter().map(|memory| json!({"id": memory.id, "content": memory.content, "source": memory.source})).collect::<Vec<_>>()})
        }
        "memory.forget" => {
            let query = string_arg(args, "query")?;
            let matches = environment.memory.search_memories(query, 100)?;
            let mut deleted = Vec::new();
            for memory in matches {
                if environment.memory.forget_memory(memory.id)? {
                    deleted.push(json!({"id":memory.id, "content":memory.content}));
                }
            }
            json!({"deleted":deleted, "query":query})
        }
        "file.read_text" => {
            let path = Path::new(string_arg(args, "path")?);
            let file = attachments::read_text_file(path)?;
            json!({"title": file.title, "source": file.source, "content": file.content, "size_chars": file.content.chars().count()})
        }
        "face.set_state" => {
            let state = string_arg(args, "state")?.to_owned();
            let caption = args
                .get("caption")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            return Ok(ToolOutput {
                result: json!({"state": state, "caption": caption}),
                face_state: Some((state, caption)),
            });
        }
        name => anyhow::bail!("tool {name:?} has no built-in executor"),
    };
    Ok(ToolOutput {
        result,
        face_state: None,
    })
}

fn string_arg<'a>(args: &'a serde_json::Map<String, Value>, name: &str) -> anyhow::Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("argument {name:?} must be a non-empty string"))
}

fn validate_schema(arguments: &Value, schema: &Value) -> anyhow::Result<()> {
    let object = arguments
        .as_object()
        .context("tool arguments must be a JSON object")?;
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            anyhow::ensure!(
                object.contains_key(name),
                "missing required argument {name:?}"
            );
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, value) in object {
            let property = properties
                .get(name)
                .with_context(|| format!("unknown argument {name:?}"))?;
            if let Some(expected) = property.get("type").and_then(Value::as_str) {
                let valid = matches!(
                    (expected, value),
                    ("string", Value::String(_))
                        | ("number", Value::Number(_))
                        | ("boolean", Value::Bool(_))
                        | ("object", Value::Object(_))
                        | ("array", Value::Array(_))
                );
                anyhow::ensure!(valid, "argument {name:?} must have type {expected}");
            }
        }
    }
    Ok(())
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
}

fn definition(
    name: &str,
    description: &str,
    schema: Value,
    risk_level: RiskLevel,
    requires_confirmation: bool,
    read_only: bool,
) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        input_schema_json: schema,
        output_schema_json: None,
        risk_level,
        requires_confirmation,
        local_only: true,
        read_only,
    }
}

fn builtin_definitions() -> Vec<ToolDefinition> {
    let empty = || object_schema(json!({}), &[]);
    vec![
        definition(
            "time.now",
            "Return the local system timestamp.",
            empty(),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "context.list",
            "List current context item summaries.",
            empty(),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "context.get",
            "Read one current context item by id.",
            object_schema(json!({"id":{"type":"string"}}), &["id"]),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "active_window.get",
            "Read active-window metadata.",
            empty(),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "clipboard.read",
            "Read text from the system clipboard.",
            empty(),
            RiskLevel::Medium,
            false,
            true,
        ),
        definition(
            "memory.remember",
            "Store explicit long-term user memory.",
            object_schema(json!({"content":{"type":"string"}}), &["content"]),
            RiskLevel::Medium,
            false,
            false,
        ),
        definition(
            "memory.search",
            "Search local user memories by text.",
            object_schema(json!({"query":{"type":"string"}}), &["query"]),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "memory.list",
            "List recent explicit long-term user memories.",
            empty(),
            RiskLevel::Low,
            false,
            true,
        ),
        definition(
            "memory.forget",
            "Soft-delete explicit memories matching a text query.",
            object_schema(json!({"query":{"type":"string"}}), &["query"]),
            RiskLevel::Medium,
            false,
            false,
        ),
        definition(
            "face.set_state",
            "Set a face state and optional caption.",
            object_schema(
                json!({"state":{"type":"string"},"caption":{"type":"string"}}),
                &["state"],
            ),
            RiskLevel::Low,
            false,
            false,
        ),
        definition(
            "file.read_text",
            "Read one local UTF-8 text file up to 64 KB.",
            object_schema(json!({"path":{"type":"string"}}), &["path"]),
            RiskLevel::Medium,
            false,
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::active_window::ActiveWindowInfo;
    use crate::memory::SqliteMemoryStore;

    struct FakeWindow;
    impl ActiveWindowProvider for FakeWindow {
        fn get_active_window(&self) -> anyhow::Result<ActiveWindowInfo> {
            Ok(ActiveWindowInfo {
                title: "Editor".into(),
                process_name: Some("test".into()),
                process_id: Some(1),
                platform: "test".into(),
            })
        }
        fn supported(&self) -> bool {
            true
        }
    }
    struct FakeClipboard;
    impl ClipboardProvider for FakeClipboard {
        fn read_text(&self) -> anyhow::Result<String> {
            Ok("clip".into())
        }
    }

    #[test]
    fn registry_rejects_duplicates_and_validates_schema() {
        let mut registry = ToolRegistry::with_builtins();
        let duplicate = registry.get("time.now").unwrap().clone();
        assert!(registry.register(duplicate).is_err());
        assert!(registry
            .validate("memory.search", &json!({"query":"Orbital"}))
            .is_ok());
        assert!(registry.validate("memory.search", &json!({})).is_err());
        assert!(registry
            .validate("time.now", &json!({"extra":true}))
            .is_err());
    }

    #[test]
    fn permissions_are_explicit_and_local_only() {
        let registry = ToolRegistry::with_builtins();
        assert_eq!(
            permission_for(registry.get("time.now").unwrap(), ToolSource::Manual),
            ToolPermission::Execute
        );
        assert_eq!(
            permission_for(
                registry.get("time.now").unwrap(),
                ToolSource::ModelSuggested
            ),
            ToolPermission::Execute
        );
        assert_eq!(
            permission_for(registry.get("file.read_text").unwrap(), ToolSource::Manual),
            ToolPermission::Execute
        );
        assert_eq!(
            permission_for(registry.get("clipboard.read").unwrap(), ToolSource::Manual),
            ToolPermission::Execute
        );
        let mut remote = registry.get("time.now").unwrap().clone();
        remote.local_only = false;
        assert_eq!(
            permission_for(&remote, ToolSource::Manual),
            ToolPermission::Deny
        );
        let mut high = registry.get("time.now").unwrap().clone();
        high.risk_level = RiskLevel::High;
        assert_eq!(
            permission_for(&high, ToolSource::ModelSuggested),
            ToolPermission::Confirm
        );
    }

    #[test]
    fn builtins_execute_with_fake_dependencies() {
        let memory = SqliteMemoryStore::open(":memory:").unwrap();
        let mut context = ContextManager::default();
        let context_id = context
            .attach_text("diagnostic context")
            .unwrap()
            .id
            .clone();
        let environment = ToolEnvironment {
            context: &context,
            memory: &memory,
            active_window: &FakeWindow,
            clipboard: &FakeClipboard,
        };
        let now = execute_tool(
            &ToolInvocation {
                tool_name: "time.now".into(),
                arguments: json!({}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert!(now.result["unix_timestamp"].is_number());
        let listed = execute_tool(
            &ToolInvocation {
                tool_name: "context.list".into(),
                arguments: json!({}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(listed.result["items"].as_array().unwrap().len(), 1);
        let fetched = execute_tool(
            &ToolInvocation {
                tool_name: "context.get".into(),
                arguments: json!({"id":context_id}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(fetched.result["content"], "diagnostic context");
        memory
            .remember("TaskRunMessageProcessor issue", "test")
            .unwrap();
        let found = execute_tool(
            &ToolInvocation {
                tool_name: "memory.search".into(),
                arguments: json!({"query":"TaskRun"}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(found.result["memories"].as_array().unwrap().len(), 1);
        let listed_memories = execute_tool(
            &ToolInvocation {
                tool_name: "memory.list".into(),
                arguments: json!({}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(
            listed_memories.result["memories"].as_array().unwrap().len(),
            1
        );
        let forgotten = execute_tool(
            &ToolInvocation {
                tool_name: "memory.forget".into(),
                arguments: json!({"query":"TaskRun"}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(forgotten.result["deleted"].as_array().unwrap().len(), 1);
        assert!(memory.list_memories(10).unwrap().is_empty());
        for name in ["Old Test Name", "TestUser"] {
            execute_tool(
                &ToolInvocation {
                    tool_name: "memory.remember".into(),
                    arguments: json!({"content":format!("User's name is {name}.")}),
                    source: ToolSource::Manual,
                },
                &environment,
            )
            .unwrap();
        }
        let names = memory.list_memories(10).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].content, "User's name is TestUser.");
        let clipboard = execute_tool(
            &ToolInvocation {
                tool_name: "clipboard.read".into(),
                arguments: json!({}),
                source: ToolSource::Manual,
            },
            &environment,
        )
        .unwrap();
        assert_eq!(clipboard.result["text"], "clip");
    }

    #[test]
    fn file_tool_enforces_attachment_limits_and_missing_errors() {
        let memory = SqliteMemoryStore::open(":memory:").unwrap();
        let context = ContextManager::default();
        let environment = ToolEnvironment {
            context: &context,
            memory: &memory,
            active_window: &FakeWindow,
            clipboard: &FakeClipboard,
        };
        let missing = execute_tool(
            &ToolInvocation {
                tool_name: "file.read_text".into(),
                arguments: json!({"path":"/definitely/missing"}),
                source: ToolSource::Manual,
            },
            &environment,
        );
        assert!(missing.is_err());
        let path = std::env::temp_dir().join(format!("orbital-large-tool-{}", std::process::id()));
        std::fs::write(&path, vec![b'x'; attachments::MAX_FILE_BYTES as usize + 1]).unwrap();
        let too_large = execute_tool(
            &ToolInvocation {
                tool_name: "file.read_text".into(),
                arguments: json!({"path":path}),
                source: ToolSource::Manual,
            },
            &environment,
        );
        assert!(too_large.unwrap_err().to_string().contains("64 KB"));
    }
}
