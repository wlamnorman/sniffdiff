use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use serde::Serialize;

pub(crate) const INDEX_REF: &str = ":index";
pub(crate) const WORKTREE_REF: &str = ":worktree";

const GIT_NOT_FOUND_MESSAGE: &str = "`git` was not found on PATH. sniffdiff requires Git to read diffs and file contents; install Git and try again.";

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

    pub(crate) fn merge_base(&self, left: &str, right: &str) -> Result<String> {
        Ok(self.git(&["merge-base", left, right])?.trim().to_string())
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = self.run_git(args)?;

        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn run_git(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .map_err(|error| git_spawn_error(args, error))
    }
}

impl GitBackend for ShellGit {
    fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>> {
        if base == INDEX_REF && head == WORKTREE_REF {
            return self.changed_files_between_index_and_worktree();
        }
        if head == WORKTREE_REF {
            return self.changed_files_against_worktree(base);
        }
        if head == INDEX_REF {
            return self.changed_files_against_index(base);
        }

        let range = format!("{base}..{head}");
        let output = self.git(&["diff", "--name-status", "--find-renames", &range])?;

        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_name_status_line)
            .collect()
    }

    fn list_files_at_ref(&self, git_ref: &str, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        if git_ref == INDEX_REF {
            return self.list_files_in_index(extensions);
        }
        if git_ref == WORKTREE_REF {
            return self.list_files_in_worktree(extensions);
        }

        let output = self.git(&["ls-tree", "-r", "--name-only", git_ref])?;
        Ok(output
            .lines()
            .map(PathBuf::from)
            .filter(|path| has_extension(path, extensions))
            .collect())
    }

    fn read_file_at_ref(&self, git_ref: &str, path: &Path) -> Result<Option<String>> {
        if git_ref == INDEX_REF {
            return self.read_file_in_index(path);
        }
        if git_ref == WORKTREE_REF {
            return self.read_file_in_worktree(path);
        }

        let spec = format!("{}:{}", git_ref, path.display());
        let output = self.run_git(&["show", &spec])?;

        if output.status.success() {
            return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
        }

        if is_missing_git_object_error(&output.stderr) {
            return Ok(None);
        }

        bail!(
            "git show {} failed: {}",
            spec,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

impl ShellGit {
    fn changed_files_between_index_and_worktree(&self) -> Result<Vec<ChangedFile>> {
        self.changed_files_from_diff(&["diff", "--name-status", "--find-renames"])
    }

    fn changed_files_against_worktree(&self, base: &str) -> Result<Vec<ChangedFile>> {
        self.changed_files_from_diff(&["diff", "--name-status", "--find-renames", base])
    }

    fn changed_files_against_index(&self, base: &str) -> Result<Vec<ChangedFile>> {
        self.changed_files_from_diff(&["diff", "--name-status", "--find-renames", "--cached", base])
    }

    fn changed_files_from_diff(&self, args: &[&str]) -> Result<Vec<ChangedFile>> {
        let output = self.git(args)?;

        output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(parse_name_status_line)
            .collect()
    }

    fn list_files_in_index(&self, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        let output = self.git(&["ls-files", "--cached"])?;
        Ok(output
            .lines()
            .map(PathBuf::from)
            .filter(|path| has_extension(path, extensions))
            .collect())
    }

    fn list_files_in_worktree(&self, extensions: &[&str]) -> Result<Vec<PathBuf>> {
        let output = self.git(&["ls-files", "--cached"])?;
        Ok(output
            .lines()
            .map(PathBuf::from)
            .filter(|path| has_extension(path, extensions))
            .filter(|path| self.repo_root.join(path).is_file())
            .collect())
    }

    fn read_file_in_worktree(&self, path: &Path) -> Result<Option<String>> {
        let path = self.repo_root.join(path);
        if !path.exists() {
            return Ok(None);
        }

        fs::read_to_string(&path)
            .with_context(|| format!("failed to read working tree file {}", path.display()))
            .map(Some)
    }

    fn read_file_in_index(&self, path: &Path) -> Result<Option<String>> {
        let spec = format!(":{}", path.display());
        let output = self.run_git(&["show", &spec])?;

        if output.status.success() {
            return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
        }

        if is_missing_git_object_error(&output.stderr) {
            return Ok(None);
        }

        bail!(
            "git show {} failed: {}",
            spec,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
}

fn is_missing_git_object_error(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("exists on disk, but not in")
        || stderr.contains("exists on disk, but not in the index")
        || stderr.contains("pathspec")
        || stderr.contains("does not exist")
}

fn git_spawn_error(args: &[&str], error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::NotFound {
        anyhow::anyhow!(GIT_NOT_FOUND_MESSAGE)
    } else {
        anyhow::anyhow!("failed to run git {}: {}", args.join(" "), error)
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

    #[test]
    fn explains_missing_git_prerequisite() {
        let error = git_spawn_error(
            &["diff"],
            io::Error::new(io::ErrorKind::NotFound, "No such file or directory"),
        );

        assert_eq!(error.to_string(), GIT_NOT_FOUND_MESSAGE);
    }
}
