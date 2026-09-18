//! Include the startup-cached package release version in executor metadata.

use codex_build_info::BuildInfo;

use crate::protocol::EnvironmentInfo;

pub(super) fn local_environment_info() -> EnvironmentInfo {
    EnvironmentInfo {
        executor_version: BuildInfo::get().version().to_string(),
        ..EnvironmentInfo::local()
    }
}
