//! Restores owner-scoped uninstall notifications and ties file cleanup to pinned directories.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use codex_windows_sandbox::clean_up_packaged_windows_sandbox;
use codex_windows_sandbox::string_from_sid_bytes;
use windows::ApplicationModel::Package;
use windows::ApplicationModel::PackageCatalog;
use windows::ApplicationModel::PackageUninstallingEventArgs;
use windows::Foundation::EventRegistrationToken;
use windows::Foundation::TypedEventHandler;
use windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;
use windows::Win32::System::WinRT::RoInitialize;
use windows::Win32::System::WinRT::RoUninitialize;
use windows::core::HSTRING;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security as security;
use windows_sys::Win32::System::RemoteDesktop::WTS_CURRENT_SERVER_HANDLE;
use windows_sys::Win32::System::RemoteDesktop::WTSEnumerateSessionsW;
use windows_sys::Win32::System::RemoteDesktop::WTSFreeMemory;
use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;

use crate::installation_record::DesktopInstallation;
use crate::installation_record::InstallationRecord;
use crate::ipc::OwnedHandle;

struct UserInstallation {
    codex_home: Option<PathBuf>,
    directory_handles: Vec<OwnedHandle>,
    desktop_installation: Option<DesktopInstallation>,
    user_token: OwnedHandle,
    catalog: PackageCatalog,
    token: EventRegistrationToken,
}

pub(crate) struct PackageLifecycle {
    package_name: HSTRING,
    uninstalling: Arc<AtomicBool>,
    installation: RefCell<Option<UserInstallation>>,
}

impl PackageLifecycle {
    pub(crate) fn new(uninstalling: Arc<AtomicBool>) -> Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .context("initialize the sandbox service Windows Runtime apartment")?;
        Ok(Self {
            package_name: Package::Current()?.Id()?.FullName()?,
            uninstalling,
            installation: RefCell::default(),
        })
    }

    pub(crate) fn watch_authenticated_user(
        &self,
        installation: &InstallationRecord,
        user_token: OwnedHandle,
    ) -> Result<()> {
        if self.installation.borrow().is_some() {
            return Ok(());
        }

        with_owner_impersonation(user_token.0, || {
            let mut directory_handles = Vec::new();
            let codex_home = match crate::ipc::pin_existing_ancestors(
                &installation.codex_home,
                &mut directory_handles,
            ) {
                Ok(()) => Some(installation.codex_home.clone()),
                Err(error) => {
                    directory_handles.clear();
                    crate::service::log_error(
                        crate::service::EVENT_SERVICE_FAILED,
                        &format!(
                            "skipping sandbox file cleanup because the home could not be pinned: {error:#}"
                        ),
                    );
                    None
                }
            };
            let catalog = PackageCatalog::OpenForCurrentUser()
                .context("open the authenticated user's package catalog")?;
            let package_name = self.package_name.clone();
            let uninstalling = Arc::clone(&self.uninstalling);
            let token = catalog
                .PackageUninstalling(&TypedEventHandler::<
                    PackageCatalog,
                    PackageUninstallingEventArgs,
                >::new(move |_, event| {
                    if let Some(event) = event
                        && event.Package()?.Id()?.FullName()? == package_name
                    {
                        uninstalling.store(!event.IsComplete()?, Ordering::Release);
                    }
                    Ok(())
                }))
                .context("subscribe to authenticated package uninstall notifications")?;
            self.installation.replace(Some(UserInstallation {
                codex_home,
                directory_handles,
                desktop_installation: installation.desktop_installation.clone(),
                user_token,
                catalog,
                token,
            }));
            Ok(())
        })
    }

    pub(crate) fn restore_logged_in_owner(&self, recorded_session_id: u32) -> Result<()> {
        let Err(error) = self.restore_authenticated_user(recorded_session_id) else {
            return Ok(());
        };

        // Session IDs can change while the service is stopped; the owner SID is durable.
        let mut sessions = std::ptr::null_mut();
        let mut count = 0;
        if unsafe {
            WTSEnumerateSessionsW(
                WTS_CURRENT_SERVER_HANDLE,
                /*reserved*/ 0,
                /*version*/ 1,
                &mut sessions,
                &mut count,
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("list current Windows sessions");
        }
        let restored = (0..count).any(|index| {
            let session_id = unsafe { (*sessions.add(index as usize)).SessionId };
            session_id != recorded_session_id && self.restore_authenticated_user(session_id).is_ok()
        });
        unsafe { WTSFreeMemory(sessions.cast()) };
        if restored { Ok(()) } else { Err(error) }
    }

    pub(crate) fn restore_authenticated_user(&self, session_id: u32) -> Result<()> {
        if self.installation.borrow().is_some() {
            return Ok(());
        }
        let Some(record) = crate::installation_record::load()? else {
            return Ok(());
        };

        let mut raw_token = 0;
        if unsafe { WTSQueryUserToken(session_id, &mut raw_token) } == 0 {
            return Err(io::Error::last_os_error()).context("open the logged-in user's token");
        }
        let token = crate::ipc::OwnedHandle(raw_token);
        let user = crate::package_identity::token_user(token.0)?;
        let sid = unsafe { std::ptr::read_unaligned(user.as_ptr().cast::<security::TOKEN_USER>()) }
            .User
            .Sid;
        let sid_length = unsafe { security::GetLengthSid(sid) };
        if sid_length == 0 {
            return Err(io::Error::last_os_error()).context("read the logged-in user's SID");
        }
        let user_sid = string_from_sid_bytes(unsafe {
            std::slice::from_raw_parts(sid.cast::<u8>(), sid_length as usize)
        })
        .map_err(anyhow::Error::msg)?;
        ensure!(
            user_sid == record.user_sid,
            "logged-in user does not match the recorded sandbox owner"
        );

        self.watch_authenticated_user(&record, token)?;
        if session_id != record.session_id {
            crate::installation_record::save(&crate::installation_record::InstallationRecord {
                session_id,
                ..record
            })?;
        }
        Ok(())
    }

    pub(crate) fn clean_up(&self) -> Result<()> {
        let mut installation = self.installation.borrow_mut();
        let installation = installation
            .as_mut()
            .context("the authenticated package installation was not recorded")?;
        crate::service::log_information(
            crate::service::EVENT_CLEANUP_STARTED,
            "sandbox uninstall cleanup started",
        );
        // A partial uninstall must not let a later install inherit stale directory ownership.
        crate::installation_record::remove()?;
        let codex_home = installation.codex_home.clone();
        clean_up_packaged_windows_sandbox(codex_home.as_deref(), || {
            let Some(desktop) = &installation.desktop_installation else {
                return Ok(());
            };
            // The marker is user-writable. It must never authorize deletion as LocalSystem.
            with_owner_impersonation(installation.user_token.0, || {
                let mut errors = Vec::new();
                let mut record_error = |result: io::Result<()>| {
                    if let Err(error) = result
                        && error.kind() != io::ErrorKind::NotFound
                    {
                        errors.push(error.to_string());
                    }
                };
                if let Some(home) = &codex_home
                    && desktop.created_codex_home
                {
                    // Release the home itself so it can be deleted; keep its ancestors pinned.
                    installation.directory_handles.pop();
                    record_error(std::fs::remove_dir_all(home));
                }
                // The cache may have been created after provisioning. Pin it only for cleanup.
                let mut cache_directory_handles = Vec::new();
                if desktop.cache_home.is_dir() {
                    match crate::ipc::pin_existing_ancestors(
                        &desktop.cache_home,
                        &mut cache_directory_handles,
                    ) {
                        Ok(()) => record_error(std::fs::remove_dir_all(
                            desktop.cache_home.join("codex-runtimes"),
                        )),
                        Err(error) => errors.push(error.to_string()),
                    }
                }
                ensure!(
                    errors.is_empty(),
                    "remove desktop directories: {}",
                    errors.join("; ")
                );
                Ok(())
            })
        })
        .context("remove packaged Windows sandbox resources")?;
        crate::service::log_information(
            crate::service::EVENT_CLEANUP_FINISHED,
            "sandbox uninstall cleanup finished",
        );
        Ok(())
    }
}

fn with_owner_impersonation(
    user_token: HANDLE,
    operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if unsafe { security::ImpersonateLoggedOnUser(user_token) } == 0 {
        return Err(io::Error::last_os_error()).context("impersonate the sandbox owner");
    }
    let result = operation();
    if unsafe { security::RevertToSelf() } == 0 {
        crate::service::log_error(
            crate::service::EVENT_SERVICE_FAILED,
            &format!(
                "unable to revert sandbox-owner impersonation: {}",
                io::Error::last_os_error()
            ),
        );
        // Continuing as the user would make later machine cleanup unsafe.
        std::process::abort();
    }
    result
}

impl Drop for PackageLifecycle {
    fn drop(&mut self) {
        if let Some(installation) = self.installation.get_mut().take() {
            let _ = installation
                .catalog
                .RemovePackageUninstalling(installation.token);
        }
        unsafe { RoUninitialize() };
    }
}
