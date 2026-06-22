use crate::context::ContextItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub items: Vec<ContextItem>,
    pub total_chars: usize,
    pub summary_for_status: String,
    pub formatted: String,
    pub truncated: bool,
}

impl PromptContext {
    pub fn from_items(items: &[ContextItem], max_chars: usize) -> Self {
        let total_chars = items.iter().map(|item| item.size_chars).sum();
        let summary_for_status = if items.is_empty() {
            "No context attached".into()
        } else {
            format!(
                "{} item(s), {} source character(s)",
                items.len(),
                total_chars
            )
        };
        if items.is_empty() || max_chars == 0 {
            return Self {
                items: items.to_vec(),
                total_chars,
                summary_for_status,
                formatted: String::new(),
                truncated: !items.is_empty() && max_chars == 0,
            };
        }

        let mut formatted = String::from("[Orbital Context]\n");
        let mut truncated = false;
        for (index, item) in items.iter().enumerate() {
            let header = format!(
                "Context item {}:\nType: {}\nTitle: {}\nSource: {}\nContent:\n",
                index + 1,
                item.kind.as_str(),
                item.title,
                item.source
            );
            if formatted.chars().count() + header.chars().count() >= max_chars {
                truncated = true;
                break;
            }
            formatted.push_str(&header);
            let remaining = max_chars.saturating_sub(formatted.chars().count());
            let content_chars: Vec<_> = item.content.chars().collect();
            if content_chars.len() + 2 > remaining {
                let reserve = "\n[Context truncated]\n".chars().count();
                let take = remaining.saturating_sub(reserve);
                formatted.extend(content_chars.into_iter().take(take));
                formatted.push_str("\n[Context truncated]\n");
                truncated = true;
                break;
            }
            formatted.push_str(&item.content);
            formatted.push_str("\n\n");
        }

        Self {
            items: items.to_vec(),
            total_chars,
            summary_for_status,
            formatted,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextItem, ContextKind};

    #[test]
    fn formats_context_block() {
        let item = ContextItem::new(
            "1",
            ContextKind::Clipboard,
            "Clipboard",
            "build failed",
            "clipboard",
        );
        let prompt = PromptContext::from_items(&[item], 1_000);
        assert!(prompt.formatted.starts_with("[Orbital Context]"));
        assert!(prompt.formatted.contains("Type: clipboard"));
        assert!(prompt.formatted.contains("build failed"));
    }

    #[test]
    fn truncates_context_to_character_budget() {
        let item = ContextItem::new(
            "1",
            ContextKind::AttachedText,
            "Long",
            "x".repeat(500),
            "test",
        );
        let prompt = PromptContext::from_items(&[item], 120);
        assert!(prompt.truncated);
        assert!(prompt.formatted.chars().count() <= 120);
        assert!(prompt.formatted.contains("[Context truncated]"));
    }

    #[test]
    fn selected_text_format_includes_title_and_source() {
        let item = ContextItem::new(
            "selection-1",
            ContextKind::SelectedText,
            "Selected Text",
            "missing class Foo",
            "Code.exe - main.rs",
        );
        let prompt = PromptContext::from_items(&[item], 1_000);
        assert!(prompt.formatted.contains("Type: selected_text"));
        assert!(prompt.formatted.contains("Title: Selected Text"));
        assert!(prompt.formatted.contains("Source: Code.exe - main.rs"));
    }
}
