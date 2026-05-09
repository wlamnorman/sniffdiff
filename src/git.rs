use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed { old_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ChangedFile {
    pub(crate) path: PathBuf,
    pub(crate) status: FileStatus,
}

pub(crate) trait GitBackend {
    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>>;
    fn list_files_at_ref(&self, git_ref: &str, extensions: &[&str]) -> Result<Vec<PathBuf>>;
    fn read_file_at_ref(&self, git_ref: &str, path: &Path) -> Result<Option<String>>;
}

#[derive(Debug, Clone)]
pub(crate) struct ShellGit {
    repo_root: PathBuf,
}

impl ShellGit {
    pub(crate) fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .with_context(|| format!("failed to run git {}", args.join(" ")))?;

        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl GitBackend for ShellGit {
    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>> {
        let range = format!("{base}..{head}");
        let output = self.git(&["diff", "--name-status", "--find-renames", &range])?;

        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_name_status_line)
            .collect()
    }

    fn list_files_at_ref(&self, git_ref: &str, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        let output = self.git(&["ls-tree", "-r", "--name-only", git_ref])?;
        Ok(output
            .lines()
            .map(PathBuf::from)
            .filter(|path| has_extension(path, extensions))
            .collect())
    }

    fn read_file_at_ref(&self, git_ref: &str, path: &Path) -> Result<Option<String>> {
        let spec = format!("{}:{}", git_ref, path.display());
        let output = Command::new("git")
            .args(["show", &spec])
            .current_dir(&self.repo_root)
            .output()
            .with_context(|| format!("failed to run git show {spec}"))?;

        if output.status.success() {
            return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
        }

        if String::from_utf8_lossy(&output.stderr).contains("exists on disk, but not in") {
            return Ok(None);
        }

        bail!(
            "git show {} failed: {}",
            spec,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn parse_name_status_line(line: &str) -> Result<ChangedFile> {
    let columns = line.split('\t').collect::<Vec<_>>();
    let status = columns
        .first()
        .context("git name-status line did not include a status")?;

    match status.chars().next() {
        Some('A') => Ok(ChangedFile {
            path: PathBuf::from(columns.get(1).context("added file missing path")?),
            status: FileStatus::Added,
        }),
        Some('M') => Ok(ChangedFile {
            path: PathBuf::from(columns.get(1).context("modified file missing path")?),
            status: FileStatus::Modified,
        }),
        Some('D') => Ok(ChangedFile {
            path: PathBuf::from(columns.get(1).context("deleted file missing path")?),
            status: FileStatus::Deleted,
        }),
        Some('R') => Ok(ChangedFile {
            path: PathBuf::from(columns.get(2).context("renamed file missing new path")?),
            status: FileStatus::Renamed {
                old_path: PathBuf::from(columns.get(1).context("renamed file missing old path")?),
            },
        }),
        _ => bail!("unsupported git name-status line: {line}"),
    }
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extensions.contains(&extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_added_modified_deleted_files() {
        assert_eq!(
            parse_name_status_line("A\tsrc/new.py").unwrap(),
            ChangedFile {
                path: PathBuf::from("src/new.py"),
                status: FileStatus::Added,
            }
        );
        assert_eq!(
            parse_name_status_line("M\tsrc/changed.py").unwrap(),
            ChangedFile {
                path: PathBuf::from("src/changed.py"),
                status: FileStatus::Modified,
            }
        );
        assert_eq!(
            parse_name_status_line("D\tsrc/old.py").unwrap(),
            ChangedFile {
                path: PathBuf::from("src/old.py"),
                status: FileStatus::Deleted,
            }
        );
    }

    #[test]
    fn parses_renamed_files() {
        assert_eq!(
            parse_name_status_line("R091\tsrc/old.py\tsrc/new.py").unwrap(),
            ChangedFile {
                path: PathBuf::from("src/new.py"),
                status: FileStatus::Renamed {
                    old_path: PathBuf::from("src/old.py"),
                },
            }
        );
    }

    #[test]
    fn filters_paths_by_extension() {
        assert!(has_extension(Path::new("src/app.py"), &["py"]));
        assert!(!has_extension(Path::new("src/app.rs"), &["py"]));
        assert!(!has_extension(Path::new("Makefile"), &["py"]));
    }
}
