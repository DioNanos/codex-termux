//! Inspects Git executable locations and repository metadata without running Git.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use codex_git_utils::get_git_repo_root;

use super::CheckStatus;
use super::DoctorCheck;
use super::DoctorIssue;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct GitCheckInputs {
    selected_git: Option<PathBuf>,
    git_candidates: Vec<PathBuf>,
    repo_root: Option<PathBuf>,
    git_entry: Option<String>,
}

pub(super) fn git_check(cwd: &Path) -> DoctorCheck {
    let selected_git = which::which("git").ok();
    let git_candidates = git_candidates();
    let repo_root = get_git_repo_root(cwd);

    git_check_from_inputs(GitCheckInputs {
        selected_git,
        git_candidates,
        git_entry: repo_root.as_deref().map(git_entry_summary),
        repo_root,
    })
}

fn git_check_from_inputs(inputs: GitCheckInputs) -> DoctorCheck {
    let mut details = Vec::new();
    match inputs.selected_git.as_deref() {
        Some(path) => details.push(format!("selected git: {}", path.display())),
        None => details.push("selected git: not found".to_string()),
    }
    details.push(format!("PATH git entries: {}", inputs.git_candidates.len()));
    for (index, path) in inputs.git_candidates.iter().enumerate() {
        details.push(format!("PATH git #{}: {}", index + 1, path.display()));
    }
    match inputs.repo_root.as_deref() {
        Some(root) => {
            details.push("repo detected: true".to_string());
            details.push(format!("repo root: {}", root.display()));
        }
        None => details.push("repo detected: false".to_string()),
    }
    if let Some(entry) = inputs.git_entry.as_deref() {
        details.push(format!(".git entry: {entry}"));
    }
    details.push("git execution: not inspected (PATH helpers are not executed)".to_string());

    let mut check = DoctorCheck::new(
        "git.environment",
        "git",
        CheckStatus::Ok,
        if inputs.selected_git.is_some() {
            "git executable found; execution not verified"
        } else {
            "git executable not found"
        },
    )
    .details(details);

    if inputs.selected_git.is_none() && inputs.repo_root.is_some() {
        check.status = CheckStatus::Warning;
        check.summary = "Git repository detected but git executable was not found".to_string();
        check = check.issue(
            DoctorIssue::new(
                CheckStatus::Warning,
                "Git repository detected but git executable was not found",
            )
            .expected("git available on PATH")
            .remedy("Install Git or fix PATH so Codex can inspect repository metadata.")
            .field("selected git"),
        );
    }

    check
}

fn git_candidates() -> Vec<PathBuf> {
    let Ok(candidates) = which::which_all("git") else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    candidates
        .filter(|candidate| seen.insert(candidate.clone()))
        .collect()
}

fn git_entry_summary(repo_root: &Path) -> String {
    let entry = repo_root.join(".git");
    match std::fs::metadata(&entry) {
        Ok(metadata) if metadata.is_dir() => "directory".to_string(),
        Ok(metadata) if metadata.is_file() => std::fs::read_to_string(&entry)
            .ok()
            .and_then(|contents| {
                contents
                    .strip_prefix("gitdir:")
                    .map(str::trim)
                    .map(|path| format!("file -> {path}"))
            })
            .unwrap_or_else(|| "file".to_string()),
        Ok(_) => "other".to_string(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => "missing".to_string(),
        Err(err) => format!("unreadable ({err})"),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn warns_when_git_repo_has_no_git_executable() {
        let check = git_check_from_inputs(GitCheckInputs {
            repo_root: Some(PathBuf::from("/repo")),
            ..GitCheckInputs::default()
        });

        assert_eq!(check.status, CheckStatus::Warning);
        assert_eq!(
            check.summary,
            "Git repository detected but git executable was not found"
        );
    }

    #[test]
    fn reports_git_candidates_and_repo_metadata() {
        let check = git_check_from_inputs(GitCheckInputs {
            selected_git: Some(PathBuf::from("/usr/bin/git")),
            git_candidates: vec![PathBuf::from("/usr/bin/git"), PathBuf::from("/opt/bin/git")],
            repo_root: Some(PathBuf::from("/repo")),
            git_entry: Some("directory".to_string()),
        });

        let report = super::super::DoctorReport {
            schema_version: 1,
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            overall_status: check.status,
            codex_version: "test".to_string(),
            checks: vec![check],
        };
        insta::assert_snapshot!(super::super::render_human_report(
            &report,
            super::super::HumanOutputOptions {
                show_details: true,
                show_all: false,
                ascii: true,
                color_enabled: false,
            },
        ));
    }
}
