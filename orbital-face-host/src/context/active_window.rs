#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub title: String,
    pub process: String,
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
        anyhow::ensure!(process_id != 0, "foreground window process is unavailable");

        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        anyhow::ensure!(
            !process_handle.is_null(),
            "failed to open foreground window process"
        );
        let mut process_buffer = vec![0_u16; 32_768];
        let mut process_length = process_buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process_handle,
            0,
            process_buffer.as_mut_ptr(),
            &mut process_length,
        );
        CloseHandle(process_handle);
        anyhow::ensure!(queried != 0, "failed to read foreground process name");
        let process_path = OsString::from_wide(&process_buffer[..process_length as usize]);
        let process = Path::new(&process_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_owned();

        Ok(ActiveWindowInfo {
            title: if title.trim().is_empty() {
                "(untitled)".into()
            } else {
                title
            },
            process,
        })
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn active_window_is_explicitly_unsupported_off_windows() {
        let error = super::collect().unwrap_err().to_string();
        assert!(error.contains("unsupported"));
    }
}
