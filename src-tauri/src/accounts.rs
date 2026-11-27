//! Sign-in details for online sources, kept in Windows Credential Manager (as generic
//! credentials named `achievement-shelf/<name>`), never in the settings file.

#[cfg(windows)]
mod imp {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn target(name: &str) -> HSTRING {
        HSTRING::from(format!("achievement-shelf/{name}"))
    }

    pub fn load(name: &str) -> Option<String> {
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: CredReadW fills `credential` with a buffer we free with CredFree below.
        unsafe {
            CredReadW(&target(name), CRED_TYPE_GENERIC, None, &mut credential).ok()?;
            let blob = std::slice::from_raw_parts((*credential).CredentialBlob, (*credential).CredentialBlobSize as usize);
            let secret = String::from_utf8(blob.to_vec()).ok();
            CredFree(credential.cast());
            secret
        }
    }

    pub fn save(name: &str, secret: &str) -> Result<(), String> {
        let target = target(name);
        let mut blob = secret.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr().cast_mut()),
            CredentialBlobSize: u32::try_from(blob.len()).map_err(|e| e.to_string())?,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY: every pointer in `credential` outlives the call.
        unsafe { CredWriteW(&credential, 0) }.map_err(|e| format!("cannot save the sign-in: {e}"))
    }

    pub fn clear(name: &str) {
        // SAFETY: a plain call with an owned string. A missing credential is fine.
        let _ = unsafe { CredDeleteW(&target(name), CRED_TYPE_GENERIC, None) };
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn load(_name: &str) -> Option<String> {
        None
    }

    pub fn save(_name: &str, _secret: &str) -> Result<(), String> {
        Err("signing in is only supported on Windows".into())
    }

    pub fn clear(_name: &str) {}
}

pub use imp::{clear, load, save};

#[cfg(all(test, windows))]
mod tests {
    /// Writes to the real Credential Manager, so it only runs when asked:
    /// `cargo test -- --ignored round_trips`.
    #[test]
    #[ignore]
    fn round_trips_a_secret() {
        let name = format!("test-{}", std::process::id());
        super::save(&name, "s3cret ✓").unwrap();
        assert_eq!(super::load(&name).as_deref(), Some("s3cret ✓"));
        super::clear(&name);
        assert_eq!(super::load(&name), None);
    }
}
