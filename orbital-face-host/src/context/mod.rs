pub mod active_window;
pub mod attachments;
pub mod clipboard;
pub mod prompt_context;

use crate::context::active_window::{
    ActiveWindowInfo, ActiveWindowProvider, SystemActiveWindowProvider,
};
use crate::context::prompt_context::PromptContext;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_CONTEXT_CHARS: usize = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Clipboard,
    ActiveWindow,
    AttachedText,
    AttachedFile,
    SelectedText,
    WatchTarget,
}

impl ContextKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clipboard => "clipboard",
            Self::ActiveWindow => "active_window",
            Self::AttachedText => "attached_text",
            Self::AttachedFile => "attached_file",
            Self::SelectedText => "selected_text",
            Self::WatchTarget => "watch_target",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    pub id: String,
    pub kind: ContextKind,
    pub title: String,
    pub content: String,
    pub source: String,
    pub created_at: u64,
    pub size_chars: usize,
}

impl ContextItem {
    pub fn new(
        id: impl Into<String>,
        kind: ContextKind,
        title: impl Into<String>,
        content: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let content = content.into();
        Self {
            id: id.into(),
            kind,
            title: title.into(),
            size_chars: content.chars().count(),
            content,
            source: source.into(),
            created_at: now_timestamp(),
        }
    }
}

#[derive(Debug)]
pub struct ContextManager {
    items: Vec<ContextItem>,
    watch_mode: bool,
    active_window: Option<ActiveWindowInfo>,
    last_context_refresh_at: Option<u64>,
    next_id: u64,
    max_prompt_chars: usize,
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONTEXT_CHARS)
    }
}

impl ContextManager {
    pub fn new(max_prompt_chars: usize) -> Self {
        Self {
            items: Vec::new(),
            watch_mode: false,
            active_window: None,
            last_context_refresh_at: None,
            next_id: 1,
            max_prompt_chars,
        }
    }

    pub fn items(&self) -> &[ContextItem] {
        &self.items
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn watch_mode(&self) -> bool {
        self.watch_mode
    }

    pub fn active_window(&self) -> Option<&ActiveWindowInfo> {
        self.active_window.as_ref()
    }

    pub fn note_active_window(&mut self, window: ActiveWindowInfo) {
        self.active_window = Some(window);
        self.last_context_refresh_at = Some(now_timestamp());
    }

    pub fn last_context_refresh_at(&self) -> Option<u64> {
        self.last_context_refresh_at
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.active_window = None;
        self.watch_mode = false;
        self.last_context_refresh_at = Some(now_timestamp());
    }

    pub fn attach_text(&mut self, text: impl Into<String>) -> anyhow::Result<&ContextItem> {
        let text = text.into();
        anyhow::ensure!(!text.trim().is_empty(), "attached text must not be empty");
        let id = self.next_id("text");
        self.items.push(ContextItem::new(
            id,
            ContextKind::AttachedText,
            "Attached Text",
            text,
            "terminal",
        ));
        self.last_context_refresh_at = Some(now_timestamp());
        Ok(self.items.last().expect("item was just inserted"))
    }

    pub fn attach_file(&mut self, path: &Path) -> anyhow::Result<&ContextItem> {
        let attachment = attachments::read_text_file(path)?;
        let id = self.next_id("file");
        self.items.push(ContextItem::new(
            id,
            ContextKind::AttachedFile,
            attachment.title,
            attachment.content,
            attachment.source,
        ));
        self.last_context_refresh_at = Some(now_timestamp());
        Ok(self.items.last().expect("item was just inserted"))
    }

    pub fn capture_clipboard(&mut self) -> anyhow::Result<&ContextItem> {
        let text = clipboard::read_text()?;
        self.upsert_singleton(
            ContextKind::Clipboard,
            "clipboard",
            "Clipboard",
            text,
            "system clipboard",
        );
        Ok(self.items.last().expect("clipboard item was inserted"))
    }

    pub fn capture_active_window(&mut self) -> anyhow::Result<&ActiveWindowInfo> {
        self.capture_active_window_with(&SystemActiveWindowProvider)
    }

    pub fn capture_active_window_with(
        &mut self,
        provider: &dyn ActiveWindowProvider,
    ) -> anyhow::Result<&ActiveWindowInfo> {
        let window = provider.get_active_window()?;
        self.store_active_window(window, ContextKind::ActiveWindow);
        Ok(self
            .active_window
            .as_ref()
            .expect("active window was just inserted"))
    }

    pub fn start_watch(&mut self) -> anyhow::Result<&ActiveWindowInfo> {
        self.start_watch_with(&SystemActiveWindowProvider)
    }

    pub fn start_watch_with(
        &mut self,
        provider: &dyn ActiveWindowProvider,
    ) -> anyhow::Result<&ActiveWindowInfo> {
        let window = provider.get_active_window()?;
        self.watch_mode = true;
        self.store_active_window(window, ContextKind::WatchTarget);
        Ok(self
            .active_window
            .as_ref()
            .expect("watch target was just inserted"))
    }

    pub fn stop_watch(&mut self) {
        self.watch_mode = false;
        self.items
            .retain(|item| item.kind != ContextKind::WatchTarget);
        self.last_context_refresh_at = Some(now_timestamp());
    }

    pub fn refresh_watch(&mut self) -> anyhow::Result<()> {
        self.refresh_watch_with(&SystemActiveWindowProvider)
    }

    pub fn refresh_watch_with(
        &mut self,
        provider: &dyn ActiveWindowProvider,
    ) -> anyhow::Result<()> {
        if !self.watch_mode {
            return Ok(());
        }
        let window = provider.get_active_window()?;
        self.store_active_window(window, ContextKind::WatchTarget);
        Ok(())
    }

    pub fn prompt_context(&self) -> PromptContext {
        PromptContext::from_items(&self.items, self.max_prompt_chars)
    }

    pub fn prompt_context_with(&self, extra: &ContextItem) -> PromptContext {
        let mut items = self.items.clone();
        items.push(extra.clone());
        PromptContext::from_items(&items, self.max_prompt_chars)
    }

    pub fn attach_selected_text(
        &mut self,
        text: impl Into<String>,
        source: impl Into<String>,
    ) -> anyhow::Result<&ContextItem> {
        let text = text.into();
        anyhow::ensure!(!text.trim().is_empty(), "selected text must not be empty");
        let id = self.next_id("selection");
        self.items.push(ContextItem::new(
            id,
            ContextKind::SelectedText,
            "Selected Text",
            text,
            source,
        ));
        self.last_context_refresh_at = Some(now_timestamp());
        Ok(self.items.last().expect("item was just inserted"))
    }

    pub fn selected_text_item(
        &mut self,
        text: impl Into<String>,
        source: impl Into<String>,
    ) -> anyhow::Result<ContextItem> {
        let text = text.into();
        anyhow::ensure!(!text.trim().is_empty(), "selected text must not be empty");
        Ok(ContextItem::new(
            self.next_id("selection-once"),
            ContextKind::SelectedText,
            "Selected Text",
            text,
            source,
        ))
    }

    fn store_active_window(&mut self, window: ActiveWindowInfo, kind: ContextKind) {
        let process = window.process_name.as_deref().unwrap_or("unknown");
        let pid = window
            .process_id
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".into());
        let content = format!(
            "Window title: {}\nProcess: {}\nPID: {}\nPlatform: {}",
            window.title, process, pid, window.platform
        );
        let id = match kind {
            ContextKind::WatchTarget => "watch-target",
            _ => "active-window",
        };
        self.items.retain(|item| {
            !matches!(
                item.kind,
                ContextKind::ActiveWindow | ContextKind::WatchTarget
            )
        });
        self.items.push(ContextItem::new(
            id,
            kind,
            if kind == ContextKind::WatchTarget {
                "Watched Window"
            } else {
                "Active Window"
            },
            content,
            process,
        ));
        self.active_window = Some(window);
        self.last_context_refresh_at = Some(now_timestamp());
    }

    fn upsert_singleton(
        &mut self,
        kind: ContextKind,
        id: &str,
        title: &str,
        content: String,
        source: &str,
    ) {
        self.items.retain(|item| item.kind != kind);
        self.items
            .push(ContextItem::new(id, kind, title, content, source));
        self.last_context_refresh_at = Some(now_timestamp());
    }

    fn next_id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_id);
        self.next_id += 1;
        id
    }
}

fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_item_tracks_character_size() {
        let item = ContextItem::new("1", ContextKind::AttachedText, "Test", "héllo", "test");
        assert_eq!(item.size_chars, 5);
        assert_eq!(item.kind.as_str(), "attached_text");
    }

    #[test]
    fn manager_adds_and_clears_context() {
        let mut manager = ContextManager::default();
        manager.attach_text("log output").unwrap();
        assert_eq!(manager.item_count(), 1);
        manager.clear();
        assert_eq!(manager.item_count(), 0);
        assert!(!manager.watch_mode());
    }

    #[test]
    fn watch_mode_state_can_be_disabled_without_platform_collection() {
        let mut manager = ContextManager {
            watch_mode: true,
            ..ContextManager::default()
        };
        manager.stop_watch();
        assert!(!manager.watch_mode());
    }

    struct FakeWindowProvider;

    impl ActiveWindowProvider for FakeWindowProvider {
        fn get_active_window(&self) -> anyhow::Result<ActiveWindowInfo> {
            Ok(ActiveWindowInfo {
                title: "main.rs".into(),
                process_name: Some("Code.exe".into()),
                process_id: Some(42),
                platform: "test".into(),
            })
        }

        fn supported(&self) -> bool {
            true
        }
    }

    #[test]
    fn active_window_provider_is_injectable() {
        let mut manager = ContextManager::default();
        let window = manager
            .capture_active_window_with(&FakeWindowProvider)
            .unwrap();
        assert_eq!(window.process_id, Some(42));
        assert!(manager.items()[0].content.contains("PID: 42"));
    }

    #[test]
    fn selected_text_context_item_is_created() {
        let mut manager = ContextManager::default();
        let item = manager
            .attach_selected_text("selected error", "Code.exe - main.rs")
            .unwrap();
        assert_eq!(item.kind, ContextKind::SelectedText);
        assert_eq!(item.title, "Selected Text");
    }
}
