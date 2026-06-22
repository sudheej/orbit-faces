pub fn read_text() -> anyhow::Result<String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| anyhow::anyhow!("clipboard unavailable: {error}"))?;
    let text = clipboard
        .get_text()
        .map_err(|error| anyhow::anyhow!("clipboard empty or unavailable: {error}"))?;
    anyhow::ensure!(!text.trim().is_empty(), "clipboard is empty");
    Ok(text)
}
