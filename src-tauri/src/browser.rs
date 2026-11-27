//! Opening web pages in the user's default browser.

/// Opens `url`, which must be an `https://` address the app built itself, never one a page sent.
#[cfg(windows)]
pub fn open(url: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    if !url.starts_with("https://") {
        return Err("Couldn't open your browser.".into());
    }
    // SAFETY: plain strings that outlive the call. ShellExecuteW returns a value above 32 on success.
    let result = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL) };
    if result.0 as isize > 32 { Ok(()) } else { Err("Couldn't open your browser.".into()) }
}

#[cfg(not(windows))]
pub fn open(_url: &str) -> Result<(), String> {
    Err("Couldn't open your browser.".into())
}
