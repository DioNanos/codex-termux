//! Binds provisioning requests to the packaged Codex client and its Windows user.

use std::io;
use std::mem::size_of;
use std::ptr;
#[cfg(debug_assertions)]
use std::sync::atomic::AtomicBool;
#[cfg(debug_assertions)]
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use windows_sys::Win32::Foundation as foundation;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security as security;
use windows_sys::Win32::Storage::Packaging::Appx;
use windows_sys::Win32::System::Pipes;
use windows_sys::Win32::System::Threading;

const MAX_PACKAGE_FAMILY_LENGTH: usize = 256;
const MAX_TOKEN_USER_BYTES: usize = 4096;

#[cfg(debug_assertions)]
static FOREGROUND_MODE: AtomicBool = AtomicBool::new(false);

struct OwnedHandle(HANDLE);

pub(crate) struct AuthorizedClientProcess {
    handle: OwnedHandle,
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != 0 && self.0 != foundation::INVALID_HANDLE_VALUE {
            unsafe { foundation::CloseHandle(self.0) };
        }
    }
}

#[cfg(debug_assertions)]
pub(crate) fn enable_foreground_mode() {
    FOREGROUND_MODE.store(true, Ordering::Release);
}

pub(crate) fn authorize_client_process(pipe: HANDLE) -> Result<AuthorizedClientProcess> {
    let mut process_id = 0;
    if unsafe { Pipes::GetNamedPipeClientProcessId(pipe, &mut process_id) } == 0 || process_id == 0
    {
        return Err(io::Error::last_os_error()).context("identify the provisioning client process");
    }

    let process = unsafe {
        Threading::OpenProcess(Threading::PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id)
    };
    if process == 0 {
        return Err(io::Error::last_os_error()).context("open the provisioning client process");
    }
    let process = OwnedHandle(process);

    let client_family = package_family_name(
        |length, buffer| unsafe { Appx::GetPackageFamilyName(process.0, length, buffer) },
        "provisioning client",
    )?;

    let service_family = package_family_name(
        |length, buffer| unsafe { Appx::GetCurrentPackageFamilyName(length, buffer) },
        "provisioning service",
    )?;
    match client_family {
        Some(client_family) => match service_family.as_deref() {
            Some(service_family) if client_family != service_family => {
                bail!("provisioning client package does not match the service package")
            }
            Some(_) => {}
            #[cfg(debug_assertions)]
            None if FOREGROUND_MODE.load(Ordering::Acquire)
                && is_known_codex_package_family(&client_family) => {}
            #[cfg(debug_assertions)]
            None if FOREGROUND_MODE.load(Ordering::Acquire) => {
                bail!("provisioning client does not belong to a trusted Codex package family")
            }
            None => bail!("provisioning service has no package identity"),
        },
        None if service_family.is_some() => {}
        #[cfg(debug_assertions)]
        None if FOREGROUND_MODE.load(Ordering::Acquire) => {}
        None => bail!("provisioning clients must run with an installed Codex package identity"),
    }

    Ok(AuthorizedClientProcess { handle: process })
}

pub(crate) fn authorize_client(
    process: &AuthorizedClientProcess,
    client_token: HANDLE,
) -> Result<()> {
    let mut process_token = 0;
    if unsafe {
        Threading::OpenProcessToken(process.handle.0, security::TOKEN_QUERY, &mut process_token)
    } == 0
    {
        return Err(io::Error::last_os_error())
            .context("open the provisioning client process token");
    }
    let process_token = OwnedHandle(process_token);
    let process_user = token_user(process_token.0).context("read the client process user")?;
    let impersonated_user =
        token_user(client_token).context("read the impersonated client user")?;
    let process_sid = unsafe {
        ptr::read_unaligned(process_user.as_ptr().cast::<security::TOKEN_USER>())
            .User
            .Sid
    };
    let impersonated_sid = unsafe {
        ptr::read_unaligned(impersonated_user.as_ptr().cast::<security::TOKEN_USER>())
            .User
            .Sid
    };
    if process_sid.is_null()
        || impersonated_sid.is_null()
        || unsafe { security::EqualSid(process_sid, impersonated_sid) } == 0
    {
        bail!("provisioning client process does not belong to the impersonated user");
    }

    Ok(())
}

fn package_family_name(
    mut query: impl FnMut(*mut u32, *mut u16) -> u32,
    subject: &str,
) -> Result<Option<String>> {
    let mut length = 0;
    let status = query(&mut length, ptr::null_mut());
    if status == foundation::APPMODEL_ERROR_NO_PACKAGE {
        return Ok(None);
    }
    if status != foundation::ERROR_INSUFFICIENT_BUFFER {
        return Err(io::Error::from_raw_os_error(status as i32))
            .with_context(|| format!("query the {subject} package family"));
    }
    if length == 0 || length as usize > MAX_PACKAGE_FAMILY_LENGTH {
        bail!("the {subject} package family has an invalid length");
    }

    let mut buffer = vec![0_u16; length as usize];
    let status = query(&mut length, buffer.as_mut_ptr());
    if status != foundation::ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32))
            .with_context(|| format!("read the {subject} package family"));
    }

    let value = buffer
        .get(..length as usize)
        .context("the package-family API returned an invalid length")?;
    let Some((&0, value)) = value.split_last() else {
        bail!("the {subject} package family is not null-terminated");
    };
    if value.is_empty() || value.contains(&0) {
        bail!("the {subject} package family is malformed");
    }
    Ok(Some(
        String::from_utf16(value).context("the package family contains invalid UTF-16")?,
    ))
}

pub(crate) fn token_user(token: HANDLE) -> Result<Vec<u8>> {
    let mut length = 0;
    unsafe {
        security::GetTokenInformation(token, security::TokenUser, ptr::null_mut(), 0, &mut length)
    };
    if length < size_of::<security::TOKEN_USER>() as u32 || length as usize > MAX_TOKEN_USER_BYTES {
        bail!("the Windows token returned an invalid user identity length");
    }

    let mut buffer = vec![0_u8; length as usize];
    if unsafe {
        security::GetTokenInformation(
            token,
            security::TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error()).context("read the Windows token user");
    }
    if length < size_of::<security::TOKEN_USER>() as u32 || length as usize > buffer.len() {
        bail!("the Windows token returned a malformed user identity");
    }
    Ok(buffer)
}

#[cfg(debug_assertions)]
fn is_known_codex_package_family(package_family: &str) -> bool {
    matches!(
        package_family,
        "OpenAI.Codex_3k8sg7r9htsxt"
            | "OpenAI.CodexAlpha_3k8sg7r9htsxt"
            | "OpenAI.CodexBeta_3k8sg7r9htsxt"
            | "OpenAI.CodexNightly_3k8sg7r9htsxt"
    )
}
