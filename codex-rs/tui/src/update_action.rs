#[cfg(any(not(debug_assertions), test))]
use codex_install_context::InstallContext;
#[cfg(any(not(debug_assertions), test))]
use codex_install_context::InstallMethod;
#[cfg(any(not(debug_assertions), test))]
use codex_install_context::StandalonePlatform;

/// Update action the CLI should perform after the TUI exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g @mmmbuto/codex-cli-termux@latest`.
    NpmGlobalLatest,
    /// Update via `bun install -g @mmmbuto/codex-cli-termux@latest`.
    BunGlobalLatest,
    /// Update via `vp install -g @mmmbuto/codex-cli-termux@latest`.
    VitePlusGlobalLatest,
    /// Update via `pnpm add -g @mmmbuto/codex-cli-termux@latest`.
    PnpmGlobalLatest,
    /// Redirect a detected Homebrew install to the supported fork npm package.
    BrewUpgrade,
    /// Update standalone installs via `npm install -g @mmmbuto/codex-cli-termux@latest`.
    StandaloneUnix,
    /// Update standalone installs via `npm install -g @mmmbuto/codex-cli-termux@latest`.
    StandaloneWindows,
}

impl UpdateAction {
    #[cfg(any(not(debug_assertions), test))]
    pub(crate) fn from_install_context(context: &InstallContext) -> Option<Self> {
        match &context.method {
            InstallMethod::Npm => Some(UpdateAction::NpmGlobalLatest),
            InstallMethod::Bun => Some(UpdateAction::BunGlobalLatest),
            InstallMethod::VitePlus => Some(UpdateAction::VitePlusGlobalLatest),
            InstallMethod::Pnpm => Some(UpdateAction::PnpmGlobalLatest),
            InstallMethod::Brew => Some(UpdateAction::BrewUpgrade),
            InstallMethod::Standalone { platform, .. } => Some(match platform {
                StandalonePlatform::Unix => UpdateAction::StandaloneUnix,
                StandalonePlatform::Windows => UpdateAction::StandaloneWindows,
            }),
            InstallMethod::Other => None,
        }
    }

    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"],
            ),
            UpdateAction::BunGlobalLatest => (
                "bun",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"],
            ),
            UpdateAction::VitePlusGlobalLatest => {
                ("vp", &["install", "-g", "@mmmbuto/codex-cli-termux@latest"])
            }
            UpdateAction::PnpmGlobalLatest => {
                ("pnpm", &["add", "-g", "@mmmbuto/codex-cli-termux@latest"])
            }
            // codex-termux fork: no Homebrew cask is shipped, so `brew upgrade
            // --cask codex` would pull the UPSTREAM openai cask and replace the
            // fork. Redirect to the supported npm channel, like Standalone*.
            UpdateAction::BrewUpgrade => (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"],
            ),
            UpdateAction::StandaloneUnix | UpdateAction::StandaloneWindows => (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"],
            ),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(not(debug_assertions))]
pub fn get_update_action() -> Option<UpdateAction> {
    UpdateAction::from_install_context(InstallContext::current())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn maps_install_context_to_update_action() {
        let native_release_dir =
            AbsolutePathBuf::from_absolute_path(std::env::temp_dir().join("native-release"))
                .expect("temp dir path should be absolute");

        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Other,
                package_layout: None,
            }),
            None
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Npm,
                package_layout: None,
            }),
            Some(UpdateAction::NpmGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Bun,
                package_layout: None,
            }),
            Some(UpdateAction::BunGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Pnpm,
                package_layout: None,
            }),
            Some(UpdateAction::PnpmGlobalLatest)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Brew,
                package_layout: None,
            }),
            Some(UpdateAction::BrewUpgrade)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Standalone {
                    platform: StandalonePlatform::Unix,
                    release_dir: native_release_dir.clone(),
                    resources_dir: Some(native_release_dir.join("codex-resources")),
                },
                package_layout: None,
            }),
            Some(UpdateAction::StandaloneUnix)
        );
        assert_eq!(
            UpdateAction::from_install_context(&InstallContext {
                method: InstallMethod::Standalone {
                    platform: StandalonePlatform::Windows,
                    release_dir: native_release_dir.clone(),
                    resources_dir: Some(native_release_dir.join("codex-resources")),
                },
                package_layout: None,
            }),
            Some(UpdateAction::StandaloneWindows)
        );
    }

    #[test]
    fn standalone_update_commands_use_fork_package() {
        assert_eq!(
            UpdateAction::StandaloneUnix.command_args(),
            (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"][..],
            )
        );
        assert_eq!(
            UpdateAction::StandaloneWindows.command_args(),
            (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"][..],
            )
        );
        assert_eq!(
            UpdateAction::BrewUpgrade.command_args(),
            (
                "npm",
                &["install", "-g", "@mmmbuto/codex-cli-termux@latest"][..],
            )
        );
    }
}
