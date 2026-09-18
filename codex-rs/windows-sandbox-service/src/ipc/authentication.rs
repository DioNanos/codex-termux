//! Authenticates provisioning clients and validates impersonated machine policy.

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_windows_sandbox::string_from_sid_bytes;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::path::PathBuf;
use std::ptr;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security as security;
use windows_sys::Win32::System::Pipes as pipes;
use windows_sys::Win32::System::Threading as threading;

use super::home::OwnedHandle;
use super::home::prepare_codex_home;
use super::request::ProvisioningRequest;

pub(super) struct ClientIdentity {
    pub(super) account: String,
    pub(super) codex_home: PathBuf,
    pub(super) user_sid: String,
    pub(super) session_id: u32,
    pub(super) token: OwnedHandle,
    pub(super) desktop_installation: Option<crate::installation_record::DesktopInstallation>,
    // Retained by both the service and helper throughout provisioning.
    pub(super) directory_handles: Vec<OwnedHandle>,
}

pub(super) fn authenticate_client(
    pipe: HANDLE,
    authorized_process: &crate::package_identity::AuthorizedClientProcess,
    sandbox_sid: &[u8],
    request: &ProvisioningRequest,
) -> Result<(ClientIdentity, Result<()>)> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                if unsafe { pipes::ImpersonateNamedPipeClient(pipe) } == 0 {
                    return Err(std::io::Error::last_os_error())
                        .context("impersonate provisioning client");
                }

                let identity = authenticate_impersonated_client(
                    authorized_process,
                    sandbox_sid,
                    &request.codex_home,
                )?;
                let policy_result = crate::machine_policy::validate_provisioning_settings(
                    &identity.codex_home,
                    &request.settings,
                    &request.listeners,
                    identity.token.0,
                );
                Ok((identity, policy_result))
            })
            .join()
            .map_err(|_| anyhow::anyhow!("provisioning client authentication thread panicked"))?
    })
}

fn authenticate_impersonated_client(
    authorized_process: &crate::package_identity::AuthorizedClientProcess,
    sandbox_sid: &[u8],
    requested_home: &Path,
) -> Result<ClientIdentity> {
    let mut raw_token = 0;
    if unsafe {
        threading::OpenThreadToken(
            threading::GetCurrentThread(),
            security::TOKEN_QUERY | security::TOKEN_IMPERSONATE,
            1,
            &mut raw_token,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("open provisioning client token");
    }
    let token = OwnedHandle(raw_token);
    crate::package_identity::authorize_client(authorized_process, token.0)
        .context("authorize the packaged Codex provisioning client")?;
    let mut session = 0_u32;
    let mut returned = 0_u32;
    if unsafe {
        security::GetTokenInformation(
            token.0,
            security::TokenSessionId,
            (&raw mut session).cast(),
            size_of::<u32>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("read provisioning client session");
    }
    if session == 0 {
        bail!("non-interactive service accounts cannot request provisioning");
    }

    let mut is_sandbox_member = 0;
    if unsafe {
        security::CheckTokenMembership(
            token.0,
            sandbox_sid.as_ptr() as *mut c_void,
            &mut is_sandbox_member,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("check sandbox group membership");
    }
    if is_sandbox_member != 0 {
        bail!("sandbox accounts cannot request provisioning");
    }

    let mut length = 0;
    unsafe {
        security::GetTokenInformation(
            token.0,
            security::TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        )
    };
    if length < size_of::<security::TOKEN_USER>() as u32 {
        return Err(std::io::Error::last_os_error()).context("size provisioning client identity");
    }
    let mut user = vec![0_u8; length as usize];
    if unsafe {
        security::GetTokenInformation(
            token.0,
            security::TokenUser,
            user.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("read provisioning client identity");
    }
    let sid = unsafe { ptr::read_unaligned(user.as_ptr().cast::<security::TOKEN_USER>()) }
        .User
        .Sid;
    let sid_length = unsafe { security::GetLengthSid(sid) };
    if sid_length == 0 {
        return Err(std::io::Error::last_os_error()).context("size provisioning client SID");
    }
    let user_sid = string_from_sid_bytes(unsafe {
        std::slice::from_raw_parts(sid.cast::<u8>(), sid_length as usize)
    })
    .map_err(anyhow::Error::msg)?;
    let account = account_name(sid)?;
    let (codex_home, handles) = prepare_codex_home(requested_home)?;
    let desktop_installation =
        crate::installation_record::read_desktop_installation(&codex_home, token.0)
            .inspect_err(|_| {
                crate::service::log_error(
                    crate::service::EVENT_SERVICE_FAILED,
                    "unable to read desktop directory ownership; preserving desktop directories",
                );
            })
            .ok();
    Ok(ClientIdentity {
        account,
        codex_home,
        user_sid,
        session_id: session,
        token,
        desktop_installation,
        directory_handles: handles,
    })
}

fn account_name(sid: *mut c_void) -> Result<String> {
    let mut name = [0_u16; 256];
    let mut domain = [0_u16; 256];
    let mut name_length = name.len() as u32;
    let mut domain_length = domain.len() as u32;
    let mut account_type: security::SID_NAME_USE = 0;
    if unsafe {
        security::LookupAccountSidW(
            std::ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_length,
            domain.as_mut_ptr(),
            &mut domain_length,
            &mut account_type,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("resolve provisioning client name");
    }
    if account_type != security::SidTypeUser || domain_length == 0 {
        bail!("provisioning client is not a named Windows user");
    }
    let name = String::from_utf16(&name[..name_length as usize])?;
    let domain = String::from_utf16(&domain[..domain_length as usize])?;
    Ok(format!("{domain}\\{name}"))
}
