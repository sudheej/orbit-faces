pub trait ClipboardProvider {
    fn read_text(&self) -> anyhow::Result<String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClipboardProvider;

impl ClipboardProvider for SystemClipboardProvider {
    fn read_text(&self) -> anyhow::Result<String> {
        read_text()
    }
}

pub fn read_text() -> anyhow::Result<String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| anyhow::anyhow!("clipboard unavailable: {error}"))?;
    let text = clipboard
        .get_text()
        .map_err(|error| anyhow::anyhow!("clipboard empty or unavailable: {error}"))?;
    anyhow::ensure!(!text.trim().is_empty(), "clipboard is empty");
    Ok(text)
}
