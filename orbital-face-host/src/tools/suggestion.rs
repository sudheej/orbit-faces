use serde::Deserialize;
use serde_json::Value;

const OPEN: &str = "```orbital_tool\n";
const CLOSE: &str = "\n```";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToolSuggestion {
    pub tool: String,
    pub arguments: Value,
}

pub fn parse_tool_suggestion(text: &str) -> Option<ToolSuggestion> {
    let start = text.find(OPEN)? + OPEN.len();
    let rest = &text[start..];
    let end = rest.find(CLOSE)?;
    if text[..start - OPEN.len()].contains("```orbital_tool")
        || rest[end + CLOSE.len()..].contains("```orbital_tool")
    {
        return None;
    }
    let suggestion: ToolSuggestion = serde_json::from_str(&rest[..end]).ok()?;
    if suggestion.tool.trim().is_empty() || !suggestion.arguments.is_object() {
        return None;
    }
    Some(suggestion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_exact_fenced_suggestion() {
        let text = "Use memory.\n```orbital_tool\n{\"tool\":\"memory.search\",\"arguments\":{\"query\":\"Orbital\"}}\n```";
        let parsed = parse_tool_suggestion(text).unwrap();
        assert_eq!(parsed.tool, "memory.search");
        assert_eq!(parsed.arguments["query"], "Orbital");
    }

    #[test]
    fn ignores_malformed_and_multiple_suggestions() {
        assert!(parse_tool_suggestion("```orbital_tool\nnope\n```").is_none());
        assert!(parse_tool_suggestion("```json\n{}\n```").is_none());
        let two = "```orbital_tool\n{\"tool\":\"time.now\",\"arguments\":{}}\n```\n```orbital_tool\n{\"tool\":\"time.now\",\"arguments\":{}}\n```";
        assert!(parse_tool_suggestion(two).is_none());
    }
}
