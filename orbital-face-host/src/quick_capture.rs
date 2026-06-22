use crate::context::active_window::ActiveWindowInfo;
#[cfg(windows)]
use crate::context::active_window::{ActiveWindowProvider, SystemActiveWindowProvider};
use crate::context::{ContextItem, ContextManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionCapture {
    pub text: String,
    pub active_window: Option<ActiveWindowInfo>,
    pub clipboard_restored: bool,
    pub warning: Option<String>,
}

pub trait SelectionProvider {
    fn capture_selection(&self) -> anyhow::Result<SelectionCapture>;
    fn supported(&self) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSelectionProvider;

impl SelectionProvider for SystemSelectionProvider {
    fn capture_selection(&self) -> anyhow::Result<SelectionCapture> {
        capture_system_selection()
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }
}

#[cfg(not(windows))]
fn capture_system_selection() -> anyhow::Result<SelectionCapture> {
    anyhow::bail!("selected-text capture is unsupported on this platform for now")
}

#[cfg(windows)]
fn capture_system_selection() -> anyhow::Result<SelectionCapture> {
    use std::mem::size_of;
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::{
        System::DataExchange::GetClipboardSequenceNumber,
        UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_C,
            VK_CONTROL,
        },
    };

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| anyhow::anyhow!("clipboard unavailable: {error}"))?;
    let previous_text = clipboard.get_text().ok();
    let sequence_before = unsafe { GetClipboardSequenceNumber() };

    let keyboard = |key, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        keyboard(VK_CONTROL, 0),
        keyboard(VK_C, 0),
        keyboard(VK_C, KEYEVENTF_KEYUP),
        keyboard(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    anyhow::ensure!(sent == inputs.len() as u32, "failed to synthesize Ctrl+C");

    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        if unsafe { GetClipboardSequenceNumber() } != sequence_before {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    anyhow::ensure!(
        unsafe { GetClipboardSequenceNumber() } != sequence_before,
        "no selection was copied"
    );

    let selected = clipboard
        .get_text()
        .map_err(|error| anyhow::anyhow!("no text selection found: {error}"))?;
    anyhow::ensure!(!selected.trim().is_empty(), "no text selection found");

    let (clipboard_restored, warning) = match previous_text {
        Some(previous) => match clipboard.set_text(previous) {
            Ok(()) => (true, None),
            Err(error) => (
                false,
                Some(format!("failed to restore previous clipboard text: {error}")),
            ),
        },
        None => (
            false,
            Some(
                "previous clipboard was empty or non-text and could not be restored; selected text remains on the clipboard"
                    .into(),
            ),
        ),
    };

    let active_window = SystemActiveWindowProvider.get_active_window().ok();
    Ok(SelectionCapture {
        text: selected,
        active_window,
        clipboard_restored,
        warning,
    })
}

pub fn selection_source(window: Option<&ActiveWindowInfo>) -> String {
    match window {
        Some(window) => {
            let process = window.process_name.as_deref().unwrap_or("unknown process");
            format!("{process} - {}", window.title)
        }
        None => "active application".into(),
    }
}

pub fn capture_context_item(
    provider: &dyn SelectionProvider,
    context: &mut ContextManager,
    persist: bool,
) -> anyhow::Result<(SelectionCapture, ContextItem)> {
    let capture = provider.capture_selection()?;
    let source = selection_source(capture.active_window.as_ref());
    if let Some(window) = capture.active_window.clone() {
        context.note_active_window(window);
    }
    let item = if persist {
        context
            .attach_selected_text(capture.text.clone(), source)?
            .clone()
    } else {
        context.selected_text_item(capture.text.clone(), source)?
    };
    Ok((capture, item))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSelectionProvider {
        result: Option<SelectionCapture>,
    }

    impl SelectionProvider for FakeSelectionProvider {
        fn capture_selection(&self) -> anyhow::Result<SelectionCapture> {
            self.result
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no selection"))
        }

        fn supported(&self) -> bool {
            true
        }
    }

    #[test]
    fn fake_provider_supports_success_and_error_paths() {
        let success = FakeSelectionProvider {
            result: Some(SelectionCapture {
                text: "selected".into(),
                active_window: None,
                clipboard_restored: true,
                warning: None,
            }),
        };
        assert_eq!(success.capture_selection().unwrap().text, "selected");

        let failure = FakeSelectionProvider { result: None };
        assert!(failure.capture_selection().is_err());
    }

    #[test]
    fn captured_selection_can_be_persisted_or_temporary() {
        let provider = FakeSelectionProvider {
            result: Some(SelectionCapture {
                text: "selected".into(),
                active_window: None,
                clipboard_restored: true,
                warning: None,
            }),
        };
        let mut context = ContextManager::default();
        let (capture, persistent) = capture_context_item(&provider, &mut context, true).unwrap();
        assert!(capture.clipboard_restored);
        assert_eq!(persistent.kind.as_str(), "selected_text");
        assert_eq!(context.item_count(), 1);

        capture_context_item(&provider, &mut context, false).unwrap();
        assert_eq!(context.item_count(), 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn system_selection_is_explicitly_unsupported_off_windows() {
        assert!(!SystemSelectionProvider.supported());
        assert!(SystemSelectionProvider.capture_selection().is_err());
    }
}
