//! Persists the authenticated sandbox owner across service restarts and package updates.

use std::ffi::OsString;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_windows_sandbox::to_wide;
use codex_windows_sandbox::validate_local_directory_path;
use serde::Deserialize;
use serde::Serialize;
use windows_sys::Win32::Foundation as foundation;
use windows_sys::Win32::System::Registry as registry;
use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

// Package updates can replace the service key, so keep this record outside it.
const INSTALLATION_KEY: &str = r"SOFTWARE\OpenAI\Codex\WindowsSandboxService";
const INSTALLATION_VALUE: &str = "ProvisionedInstallation";
const MAX_VALUE_UNITS: usize = 4096;
const DESKTOP_INSTALLATION_MARKER: &str = ".desktop-created";

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct DesktopInstallation {
    pub(crate) created_codex_home: bool,
    pub(crate) cache_home: PathBuf,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct InstallationRecord {
    pub(crate) user_sid: String,
    pub(crate) codex_home: PathBuf,
    pub(crate) session_id: u32,
    #[serde(default)]
    pub(crate) desktop_installation: Option<DesktopInstallation>,
}

// Read this app-owned marker while impersonating the authenticated user. The
// desktop writes it only when creating the home. Cache cleanup does not need ownership.
pub(crate) fn read_desktop_installation(
    home: &Path,
    user_token: foundation::HANDLE,
) -> Result<DesktopInstallation> {
    let mut length = 0;
    unsafe { GetUserProfileDirectoryW(user_token, ptr::null_mut(), &mut length) };
    let mut profile = vec![0_u16; length as usize];
    if unsafe { GetUserProfileDirectoryW(user_token, profile.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error()).context("read sandbox owner's profile directory");
    }
    let cache_home =
        PathBuf::from(OsString::from_wide(&profile[..length as usize - 1])).join(".cache");
    validate_local_directory_path(&cache_home)?;
    Ok(DesktopInstallation {
        created_codex_home: home.join(DESKTOP_INSTALLATION_MARKER).is_file(),
        cache_home,
    })
}

pub(crate) fn load() -> Result<Option<InstallationRecord>> {
    let mut value = [0_u16; MAX_VALUE_UNITS];
    let mut value_length = std::mem::size_of_val(&value) as u32;
    let status = unsafe {
        registry::RegGetValueW(
            registry::HKEY_LOCAL_MACHINE,
            to_wide(INSTALLATION_KEY).as_ptr(),
            to_wide(INSTALLATION_VALUE).as_ptr(),
            registry::RRF_RT_REG_SZ,
            ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut value_length,
        )
    };
    match status {
        foundation::ERROR_FILE_NOT_FOUND | foundation::ERROR_PATH_NOT_FOUND => return Ok(None),
        foundation::ERROR_SUCCESS => {}
        status => {
            return Err(io::Error::from_raw_os_error(status as i32))
                .context("read protected sandbox installation record");
        }
    }
    ensure!(
        value_length as usize <= std::mem::size_of_val(&value)
            && value_length.is_multiple_of(size_of::<u16>() as u32),
        "sandbox installation record has an invalid length"
    );
    let value = value[..value_length as usize / size_of::<u16>()]
        .strip_suffix(&[0])
        .context("sandbox installation record is not null-terminated")?;
    let value = String::from_utf16(value).context("decode sandbox installation record")?;
    let record = serde_json::from_str(&value).context("parse sandbox installation record")?;
    Ok(Some(record))
}

pub(crate) fn save(record: &InstallationRecord) -> Result<()> {
    let value = to_wide(
        serde_json::to_string(record).context("serialize protected sandbox installation record")?,
    );
    ensure!(
        value.len() <= MAX_VALUE_UNITS,
        "sandbox installation record exceeds its size limit"
    );
    let status = unsafe {
        registry::RegSetKeyValueW(
            registry::HKEY_LOCAL_MACHINE,
            to_wide(INSTALLATION_KEY).as_ptr(),
            to_wide(INSTALLATION_VALUE).as_ptr(),
            registry::REG_SZ,
            value.as_ptr().cast(),
            (value.len() * size_of::<u16>()) as u32,
        )
    };
    if status == foundation::ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
            .context("persist protected sandbox installation record")
    }
}

pub(crate) fn remove() -> Result<()> {
    let status = unsafe {
        registry::RegDeleteKeyW(
            registry::HKEY_LOCAL_MACHINE,
            to_wide(INSTALLATION_KEY).as_ptr(),
        )
    };
    match status {
        foundation::ERROR_SUCCESS
        | foundation::ERROR_FILE_NOT_FOUND
        | foundation::ERROR_PATH_NOT_FOUND => Ok(()),
        status => Err(io::Error::from_raw_os_error(status as i32))
            .context("remove protected sandbox installation record"),
    }
}
