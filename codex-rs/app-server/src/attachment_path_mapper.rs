use std::path::Path;
use std::path::PathBuf;

const CLIENT_ATTACHMENTS_ROOT: &str = "/tmp/codex-remote-attachments";
const ATTACHMENTS_DIRECTORY: &str = "codex-remote-attachments";

pub(crate) fn to_host_path(path: &Path) -> PathBuf {
    map_client_path(path, termux_temp_dir().as_deref())
}

pub(crate) fn to_client_path(path: &Path) -> PathBuf {
    map_host_path(path, termux_temp_dir().as_deref())
}

fn termux_temp_dir() -> Option<PathBuf> {
    std::env::var_os("TERMUX_VERSION").map(|_| std::env::temp_dir())
}

fn map_client_path(path: &Path, temp_dir: Option<&Path>) -> PathBuf {
    let Some(temp_dir) = temp_dir else {
        return path.to_path_buf();
    };
    let Ok(relative_path) = path.strip_prefix(CLIENT_ATTACHMENTS_ROOT) else {
        return path.to_path_buf();
    };
    temp_dir.join(ATTACHMENTS_DIRECTORY).join(relative_path)
}

fn map_host_path(path: &Path, temp_dir: Option<&Path>) -> PathBuf {
    let Some(temp_dir) = temp_dir else {
        return path.to_path_buf();
    };
    let host_attachments_root = temp_dir.join(ATTACHMENTS_DIRECTORY);
    let Ok(relative_path) = path.strip_prefix(host_attachments_root) else {
        return path.to_path_buf();
    };
    Path::new(CLIENT_ATTACHMENTS_ROOT).join(relative_path)
}

#[cfg(test)]
pub(crate) fn to_host_path_for_termux(path: &Path, temp_dir: &Path) -> PathBuf {
    map_client_path(path, Some(temp_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn termux_maps_only_the_attachments_directory_by_path_component() {
        let temp_dir = Path::new("/data/data/com.termux/files/usr/tmp");
        let path = Path::new("/tmp/codex-remote-attachments/image.png");

        assert_eq!(
            map_client_path(path, Some(temp_dir)),
            PathBuf::from("/data/data/com.termux/files/usr/tmp")
                .join("codex-remote-attachments")
                .join("image.png")
        );
        assert_eq!(
            map_client_path(
                Path::new("/tmp/codex-remote-attachments-other/image.png"),
                Some(temp_dir),
            ),
            PathBuf::from("/tmp/codex-remote-attachments-other/image.png")
        );
    }

    #[test]
    fn termux_watch_notifications_return_the_client_namespace() {
        let temp_dir = Path::new("/data/data/com.termux/files/usr/tmp");

        assert_eq!(
            map_host_path(
                Path::new("/data/data/com.termux/files/usr/tmp/codex-remote-attachments/image.png"),
                Some(temp_dir),
            ),
            PathBuf::from("/tmp/codex-remote-attachments/image.png")
        );
    }

    #[test]
    fn non_termux_paths_are_unchanged() {
        let path = Path::new("/tmp/codex-remote-attachments/image.png");

        assert_eq!(map_client_path(path, None), path);
        assert_eq!(map_host_path(path, None), path);
    }
}
