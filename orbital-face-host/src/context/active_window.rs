#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub title: String,
    pub process_name: Option<String>,
    pub process_id: Option<u32>,
    pub platform: String,
}

pub trait ActiveWindowProvider {
    fn get_active_window(&self) -> anyhow::Result<ActiveWindowInfo>;
    fn supported(&self) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemActiveWindowProvider;

impl ActiveWindowProvider for SystemActiveWindowProvider {
    fn get_active_window(&self) -> anyhow::Result<ActiveWindowInfo> {
        collect()
    }

    fn supported(&self) -> bool {
        cfg!(windows)
    }
}

#[cfg(not(windows))]
pub fn collect() -> anyhow::Result<ActiveWindowInfo> {
    anyhow::bail!("active-window metadata is unsupported on this platform for now")
}

#[cfg(windows)]
pub fn collect() -> anyhow::Result<ActiveWindowInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::Path;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        },
    };

    unsafe {
        let window = GetForegroundWindow();
        anyhow::ensure!(!window.is_null(), "no foreground window is available");

        let title_length = GetWindowTextLengthW(window);
        let mut title_buffer = vec![0_u16; title_length.max(0) as usize + 1];
        let copied = GetWindowTextW(window, title_buffer.as_mut_ptr(), title_buffer.len() as i32);
        let title = String::from_utf16_lossy(&title_buffer[..copied.max(0) as usize]);

        let mut process_id = 0_u32;
        GetWindowThreadProcessId(window, &mut process_id);
        let process_name = if process_id == 0 {
            None
        } else {
            let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
            if process_handle.is_null() {
                eprintln!(
                    "active-window warning: failed to open foreground process pid={process_id}"
                );
                None
            } else {
                let mut process_buffer = vec![0_u16; 32_768];
                let mut process_length = process_buffer.len() as u32;
                let queried = QueryFullProcessImageNameW(
                    process_handle,
                    0,
                    process_buffer.as_mut_ptr(),
                    &mut process_length,
                );
                CloseHandle(process_handle);
                if queried == 0 {
                    eprintln!(
                        "active-window warning: failed to query process image pid={process_id}"
                    );
                    None
                } else {
                    let process_path =
                        OsString::from_wide(&process_buffer[..process_length as usize]);
                    Path::new(&process_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                }
            }
        };

        Ok(ActiveWindowInfo {
            title: if title.trim().is_empty() {
                "(untitled)".into()
            } else {
                title
            },
            process_name,
            process_id: (process_id != 0).then_some(process_id),
            platform: "windows".into(),
        })
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::{ActiveWindowProvider, SystemActiveWindowProvider};

    #[test]
    fn active_window_is_explicitly_unsupported_off_windows() {
        let error = super::collect().unwrap_err().to_string();
        assert!(error.contains("unsupported"));
        assert!(!SystemActiveWindowProvider.supported());
    }
}
