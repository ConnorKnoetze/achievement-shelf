//! The programs running on this PC, to poll online sources faster while a game is played.

/// The lower-case executable names of every running process.
#[cfg(windows)]
pub fn running() -> Vec<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut names = Vec::new();
    // SAFETY: the snapshot handle is closed below, and `entry` is sized as the API expects.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return names };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    names
}

#[cfg(not(windows))]
pub fn running() -> Vec<String> {
    Vec::new()
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn lists_this_test_process() {
        let me = std::env::current_exe().unwrap();
        let me = me.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(super::running().contains(&me));
    }
}
