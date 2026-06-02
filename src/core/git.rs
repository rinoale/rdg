use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

use crate::core::truncate_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Unchanged,
    Changed,
    Untracked,
    Deleted,
}

impl ChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Unchanged => "clean",
            ChangeKind::Changed => "changed",
            ChangeKind::Untracked => "new",
            ChangeKind::Deleted => "deleted",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            ChangeKind::Unchanged => " ",
            ChangeKind::Changed => "M",
            ChangeKind::Untracked => "?",
            ChangeKind::Deleted => "D",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: String,
    pub kind: ChangeKind,
    pub status: String,
}

pub fn git_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        bail!(
            "not inside a git repository:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let root = String::from_utf8(output.stdout)
        .context("git root path was not valid UTF-8")?
        .trim()
        .to_string();

    Ok(PathBuf::from(root))
}

pub fn git_candidates(repo_root: &Path) -> Result<Vec<Candidate>> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .context("failed to run git status")?;

    if !output.status.success() {
        bail!(
            "git status failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let chunks: Vec<&[u8]> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .collect();

    let mut candidates = Vec::new();
    let mut i = 0;

    while i < chunks.len() {
        let entry = String::from_utf8_lossy(chunks[i]).into_owned();

        if entry.len() < 4 {
            i += 1;
            continue;
        }

        let status = entry[0..2].to_string();
        let path = entry[3..].to_string();

        let x = status.as_bytes()[0] as char;
        let y = status.as_bytes()[1] as char;

        let kind = if x == '?' && y == '?' {
            ChangeKind::Untracked
        } else if x == 'D' || y == 'D' {
            ChangeKind::Deleted
        } else {
            ChangeKind::Changed
        };

        candidates.push(Candidate {
            path,
            kind,
            status: status.clone(),
        });

        let is_rename = x == 'R' || y == 'R';
        let is_copy = x == 'C' || y == 'C';

        if (is_rename || is_copy) && i + 1 < chunks.len() {
            let old_path = String::from_utf8_lossy(chunks[i + 1]).into_owned();

            if is_rename {
                candidates.push(Candidate {
                    path: old_path,
                    kind: ChangeKind::Deleted,
                    status: String::from("R-old"),
                });
            }

            i += 1;
        }

        i += 1;
    }

    candidates.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(candidates)
}

pub fn preview_candidate(repo_root: &Path, candidate: &Candidate) -> Result<String> {
    let mut header = String::new();

    header.push_str(&format!("path:   {}\n", candidate.path));
    header.push_str(&format!("kind:   {}\n", candidate.kind.label()));
    header.push_str(&format!("status: {}\n", candidate.status));
    header.push_str(&format!("{}\n\n", "-".repeat(80)));

    let body = match candidate.kind {
        ChangeKind::Untracked => preview_untracked_file(repo_root, &candidate.path)?,
        ChangeKind::Unchanged | ChangeKind::Changed | ChangeKind::Deleted => {
            preview_git_diff(repo_root, &candidate.path)?
        }
    };

    Ok(truncate_text(format!("{header}{body}"), 200_000))
}

fn preview_git_diff(repo_root: &Path, path: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--no-ext-diff", "--color=never", "HEAD", "--"])
        .arg(path)
        .output()
        .context("failed to run git diff")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && stdout.trim().is_empty() {
        return Ok(format!("git diff failed:\n{stderr}"));
    }

    if stdout.trim().is_empty() {
        Ok(String::from(
            "No textual git diff output.\n\nThis may be a mode-only change, binary file, or a repository without HEAD.",
        ))
    } else {
        Ok(stdout)
    }
}

fn preview_untracked_file(repo_root: &Path, path: &str) -> Result<String> {
    let full_path = repo_root.join(path);

    let metadata = fs::metadata(&full_path)
        .with_context(|| format!("failed to read metadata for {}", full_path.display()))?;

    if metadata.is_dir() {
        return Ok(format!(
            "NEW DIRECTORY\n\n{}\n\nGit usually reports individual files when --untracked-files=all is used.",
            path
        ));
    }

    let bytes =
        fs::read(&full_path).with_context(|| format!("failed to read {}", full_path.display()))?;

    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return Ok(format!(
            "NEW BINARY FILE\n\n{}\n\n{} bytes",
            path,
            bytes.len()
        ));
    }

    let truncated = bytes.len() > 200_000;
    let visible_bytes = if truncated {
        &bytes[..200_000]
    } else {
        &bytes[..]
    };

    let text = String::from_utf8_lossy(visible_bytes);

    let mut preview = String::new();
    preview.push_str("NEW FILE PREVIEW\n\n");

    for line in text.lines().take(500) {
        preview.push_str("+ ");
        preview.push_str(line);
        preview.push('\n');
    }

    if truncated {
        preview.push_str("\n... file preview truncated ...\n");
    }

    Ok(preview)
}
