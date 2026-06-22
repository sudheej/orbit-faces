use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ShowStatus,
    CaptureSelection,
    AskSelectionDefault,
    ListenFiveSeconds,
}

pub trait HotkeyProvider {
    fn start(&self) -> anyhow::Result<Receiver<HotkeyAction>>;
    fn supported(&self) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemHotkeyProvider;

impl HotkeyProvider for SystemHotkeyProvider {
    fn start(&self) -> anyhow::Result<Receiver<HotkeyAction>> {
        start_system_hotkeys()
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }
}

#[cfg(not(windows))]
fn start_system_hotkeys() -> anyhow::Result<Receiver<HotkeyAction>> {
    anyhow::bail!("global hotkeys are unsupported on this platform for now")
}

#[cfg(windows)]
fn start_system_hotkeys() -> anyhow::Result<Receiver<HotkeyAction>> {
    use std::ptr::null_mut;
    use std::sync::mpsc;
    use std::thread;
    use windows_sys::Win32::UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_A, VK_O, VK_S,
        },
        WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY},
    };

    const SHOW_STATUS: i32 = 1;
    const CAPTURE_SELECTION: i32 = 2;
    const ASK_SELECTION: i32 = 3;

    let (tx, rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::channel();
    thread::spawn(move || unsafe {
        let modifiers = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        const LISTEN: i32 = 4;
        let registrations = [
            (SHOW_STATUS, VK_O as u32),
            (CAPTURE_SELECTION, VK_S as u32),
            (ASK_SELECTION, VK_A as u32),
            (
                LISTEN,
                windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_L as u32,
            ),
        ];
        let mut registered = Vec::new();
        for (id, key) in registrations {
            if RegisterHotKey(null_mut(), id, modifiers, key) == 0 {
                for registered_id in registered {
                    UnregisterHotKey(null_mut(), registered_id);
                }
                let _ = startup_tx.send(Err(anyhow::anyhow!(
                    "failed to register Ctrl+Alt hotkey id={id}"
                )));
                return;
            }
            registered.push(id);
        }
        let _ = startup_tx.send(Ok(()));

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message != WM_HOTKEY {
                continue;
            }
            let action = match message.wParam as i32 {
                SHOW_STATUS => HotkeyAction::ShowStatus,
                CAPTURE_SELECTION => HotkeyAction::CaptureSelection,
                ASK_SELECTION => HotkeyAction::AskSelectionDefault,
                LISTEN => HotkeyAction::ListenFiveSeconds,
                _ => continue,
            };
            if tx.send(action).is_err() {
                break;
            }
        }
        for id in registered {
            UnregisterHotKey(null_mut(), id);
        }
    });

    startup_rx
        .recv()
        .map_err(|_| anyhow::anyhow!("hotkey registration thread exited"))??;
    Ok(rx)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::{HotkeyProvider, SystemHotkeyProvider};

    #[test]
    fn system_hotkeys_are_disabled_off_windows() {
        assert!(!SystemHotkeyProvider.supported());
        assert!(SystemHotkeyProvider.start().is_err());
    }
}
