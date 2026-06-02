use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use tempfile::{NamedTempFile, tempdir};

use crate::core::{
    git::{Candidate, ChangeKind},
    truncate_text,
};

#[derive(Debug)]
struct RsyncCapabilities {
    version: String,
    delete_missing_args: bool,
}

#[derive(Clone, Debug)]
pub struct RsyncRunReport {
    mode: String,
    repo_root: String,
    target: String,
    selected: Vec<Candidate>,
    args: Vec<String>,
    version: String,
    delete_missing_args: bool,
    stdout: String,
    stderr: String,
    content_diffs: HashMap<String, String>,
}

impl RsyncRunReport {
    pub fn render_status(&self, focus_path: Option<&str>) -> String {
        let mut report = String::new();

        report.push_str(&format!("{}\n", self.mode));
        report.push_str(&format!("repo:   {}\n", self.repo_root));
        report.push_str(&format!("target: {}\n", self.target));
        report.push_str(&format!("files:  {}\n\n", self.selected.len()));
        report.push_str(&format!("rsync:  {}\n", self.version));
        report.push_str(&format!(
            "delete support: {}\n\n",
            if self.delete_missing_args {
                "--delete-missing-args"
            } else {
                "not available"
            }
        ));
        report.push_str("comparison: checksum\n\n");

        report.push_str(&format!(
            "$ rsync {} --files-from=<tempfile> ./ {}\n\n",
            self.args.join(" "),
            self.target
        ));

        report.push_str("CURRENT FILE RSYNC DIFF\n");
        report.push_str("-----------------------\n");
        self.push_current_file_status(&mut report, focus_path);

        report.push_str("\nSELECTED FILES\n");
        report.push_str("--------------\n");

        for candidate in &self.selected {
            report.push_str(&format!(
                "{} {:<8} {}\n",
                candidate.kind.short(),
                candidate.kind.label(),
                candidate.path
            ));
        }

        report.push_str("\nTOTAL RSYNC DIFF\n");
        report.push_str("----------------\n");

        if self.stdout.trim().is_empty() {
            report.push_str("(empty)\n");
        } else {
            report.push_str(&self.stdout);
        }

        report.push_str("\nRSYNC STDERR\n");
        report.push_str("------------\n");

        if self.stderr.trim().is_empty() {
            report.push_str("(empty)\n");
        } else {
            report.push_str(&self.stderr);
        }

        truncate_text(report, 200_000)
    }

    pub fn render_content_diff(&self, focus_path: Option<&str>) -> String {
        let mut report = String::new();
        let Some(path) = focus_path else {
            report.push_str("No current file.\n");
            return report;
        };

        report.push_str(path);
        report.push_str("\n\n");

        if !self.selected.iter().any(|candidate| candidate.path == path) {
            report.push_str(
                "Current file is not selected, so it is not included in the destination diff.\n",
            );
            return report;
        }

        let matching_lines = matching_itemized_lines(&self.stdout, path);

        if matching_lines.is_empty() {
            report.push_str("No destination content change for current file.\n");
        } else if let Some(diff) = self.content_diffs.get(path) {
            report.push_str(diff);
        } else {
            report.push_str("No textual content diff is available for this rsync change.\n");
        }

        truncate_text(report, 200_000)
    }

    fn push_current_file_status(&self, report: &mut String, focus_path: Option<&str>) {
        let Some(path) = focus_path else {
            report.push_str("(no current file)\n");
            return;
        };

        if !self.selected.iter().any(|candidate| candidate.path == path) {
            report.push_str(&format!(
                "{path}\n\nCurrent file is not selected, so it is not included in the rsync diff.\n"
            ));
            return;
        }

        let matching_lines = matching_itemized_lines(&self.stdout, path);

        report.push_str(path);
        report.push_str("\n\n");

        if matching_lines.is_empty() {
            report.push_str("(no destination change for current file)\n");
        } else {
            report.push_str("Itemized rsync change:\n");
            for line in &matching_lines {
                report.push_str(line);
                report.push('\n');
            }

            report.push_str("\nDecoded:\n");
            for line in &matching_lines {
                for description in decode_itemized_change(line) {
                    report.push_str("  - ");
                    report.push_str(&description);
                    report.push('\n');
                }
            }
        }
    }
}

pub fn rsync_selected(
    repo_root: &Path,
    target: &str,
    selected: &[Candidate],
    dry_run: bool,
) -> Result<RsyncRunReport> {
    let capabilities = rsync_capabilities()?;
    let has_deleted_files = selected
        .iter()
        .any(|candidate| candidate.kind == ChangeKind::Deleted);

    if has_deleted_files && !capabilities.delete_missing_args {
        bail!(
            "{}",
            unsupported_delete_missing_args_report(
                repo_root,
                target,
                selected,
                dry_run,
                &capabilities
            )
        );
    }

    let mut files_from = NamedTempFile::new().context("failed to create rsync file list")?;

    for candidate in selected {
        files_from
            .write_all(candidate.path.as_bytes())
            .context("failed to write path to rsync file list")?;
        files_from
            .write_all(&[0])
            .context("failed to write NUL delimiter to rsync file list")?;
    }

    files_from.flush()?;

    let mode = if dry_run { "DRY RUN" } else { "RUN" };
    let mut args = vec![
        String::from("-a"),
        String::from("-i"),
        String::from("--checksum"),
        String::from("--out-format=%i %n%L"),
        String::from("--from0"),
    ];
    let files_from_arg = format!("--files-from={}", files_from.path().display());

    if has_deleted_files {
        args.push(String::from("--delete-missing-args"));
    }

    if dry_run {
        args.push(String::from("--dry-run"));
    }

    let output = Command::new("rsync")
        .current_dir(repo_root)
        .args(&args)
        .arg(&files_from_arg)
        .arg("./")
        .arg(target)
        .stdin(Stdio::null())
        .output()
        .context("failed to execute rsync. Is rsync installed and available in PATH?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let report = RsyncRunReport {
        mode: mode.to_string(),
        repo_root: repo_root.display().to_string(),
        target: target.to_string(),
        selected: selected.to_vec(),
        args,
        version: capabilities.version,
        delete_missing_args: capabilities.delete_missing_args,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        content_diffs: if dry_run {
            content_diffs(repo_root, target, selected, &stdout)?
        } else {
            HashMap::new()
        },
    };

    if !output.status.success() {
        bail!(
            "rsync exited with status {}\n\n{}",
            output.status,
            report.render_status(None)
        );
    }

    Ok(report)
}

fn rsync_capabilities() -> Result<RsyncCapabilities> {
    let version_output = Command::new("rsync")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .context("failed to execute rsync. Is rsync installed and available in PATH?")?;

    if !version_output.status.success() {
        bail!(
            "failed to query rsync version:\n{}",
            String::from_utf8_lossy(&version_output.stderr)
        );
    }

    let version = String::from_utf8_lossy(&version_output.stdout)
        .lines()
        .next()
        .unwrap_or("rsync version unknown")
        .trim()
        .to_string();

    let help_output = Command::new("rsync")
        .arg("--help")
        .stdin(Stdio::null())
        .output()
        .context("failed to query rsync help output")?;

    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&help_output.stdout),
        String::from_utf8_lossy(&help_output.stderr)
    );

    Ok(RsyncCapabilities {
        version,
        delete_missing_args: help.contains("--delete-missing-args"),
    })
}

fn unsupported_delete_missing_args_report(
    repo_root: &Path,
    target: &str,
    selected: &[Candidate],
    dry_run: bool,
    capabilities: &RsyncCapabilities,
) -> String {
    let mode = if dry_run { "DRY RUN" } else { "RUN" };
    let mut report = String::new();

    report.push_str(&format!("{mode}\n"));
    report.push_str(&format!("repo:   {}\n", repo_root.display()));
    report.push_str(&format!("target: {target}\n"));
    report.push_str(&format!("files:  {}\n\n", selected.len()));
    report.push_str(&format!("rsync:  {}\n", capabilities.version));
    report.push_str("delete support: not available\n\n");

    report.push_str("UNSUPPORTED RSYNC DELETION\n");
    report.push_str("--------------------------\n");
    report.push_str(
        "At least one selected file is deleted in Git, but this rsync does not support \
         --delete-missing-args. rdg did not run rsync because copying the remaining files \
         while skipping deletions would leave the remote tree inconsistent.\n\n",
    );
    report.push_str(
        "Upgrade rsync on the sender and, for remote targets, on the receiver too. \
         Then rerun the dry-run before deploying.\n",
    );

    report.push_str("\nSELECTED FILES\n");
    report.push_str("--------------\n");

    for candidate in selected {
        report.push_str(&format!(
            "{} {:<8} {}\n",
            candidate.kind.short(),
            candidate.kind.label(),
            candidate.path
        ));
    }

    truncate_text(report, 200_000)
}

fn itemized_line_matches_path(line: &str, path: &str) -> bool {
    const ITEMIZE_PREFIX_LEN: usize = 12;

    if line.len() <= ITEMIZE_PREFIX_LEN {
        return false;
    }

    let itemized_path = &line[ITEMIZE_PREFIX_LEN..];

    itemized_path == path || itemized_path.starts_with(&format!("{path} ->"))
}

fn matching_itemized_lines<'a>(stdout: &'a str, path: &str) -> Vec<&'a str> {
    stdout
        .lines()
        .filter(|line| itemized_line_matches_path(line, path))
        .collect()
}

fn decode_itemized_change(line: &str) -> Vec<String> {
    const ITEMIZE_CODE_LEN: usize = 11;

    let code = line.get(..ITEMIZE_CODE_LEN).unwrap_or(line);

    if code.starts_with("*deleting") {
        return vec![String::from("destination item would be deleted")];
    }

    let chars: Vec<char> = code.chars().collect();
    let mut descriptions = Vec::new();

    match chars.first().copied() {
        Some('>') | Some('<') => descriptions.push(String::from(
            "file data would be transferred from source to destination",
        )),
        Some('c') => descriptions.push(String::from(
            "destination item would be created or locally changed",
        )),
        Some('.') => descriptions.push(String::from(
            "file data is unchanged, but attributes may change",
        )),
        Some('*') => descriptions.push(String::from("rsync emitted a special message")),
        _ => {}
    }

    match chars.get(1).copied() {
        Some('f') => descriptions.push(String::from("regular file")),
        Some('d') => descriptions.push(String::from("directory")),
        Some('L') => descriptions.push(String::from("symbolic link")),
        Some('D') => descriptions.push(String::from("device")),
        Some('S') => descriptions.push(String::from("special file")),
        _ => {}
    }

    if chars.iter().skip(2).any(|ch| *ch == '+') {
        descriptions.push(String::from("destination item is new"));
        return descriptions;
    }

    if matches!(chars.get(2), Some('c')) {
        descriptions.push(String::from("checksum/content differs"));
    }
    if matches!(chars.get(3), Some('s')) {
        descriptions.push(String::from("size differs"));
    }
    if matches!(chars.get(4), Some('t')) {
        descriptions.push(String::from("modification time differs"));
    } else if matches!(chars.get(4), Some('T')) {
        descriptions.push(String::from(
            "modification time will be set to transfer time",
        ));
    }
    if matches!(chars.get(5), Some('p')) {
        descriptions.push(String::from("permissions differ"));
    }
    if matches!(chars.get(6), Some('o')) {
        descriptions.push(String::from("owner differs"));
    }
    if matches!(chars.get(7), Some('g')) {
        descriptions.push(String::from("group differs"));
    }
    match chars.get(8).copied() {
        Some('u') => descriptions.push(String::from("access time differs")),
        Some('n') => descriptions.push(String::from("create time differs")),
        Some('b') => descriptions.push(String::from("access and create times differ")),
        _ => {}
    }
    if matches!(chars.get(9), Some('a')) {
        descriptions.push(String::from("ACL differs"));
    }
    if matches!(chars.get(10), Some('x')) {
        descriptions.push(String::from("extended attributes differ"));
    }

    descriptions
}

fn content_diffs(
    repo_root: &Path,
    target: &str,
    selected: &[Candidate],
    stdout: &str,
) -> Result<HashMap<String, String>> {
    let mut diffs = HashMap::new();
    let paths_to_fetch = paths_requiring_destination_snapshot(selected, stdout);
    let snapshot = if paths_to_fetch.is_empty() {
        None
    } else {
        Some(fetch_destination_snapshot(target, &paths_to_fetch)?)
    };

    for candidate in selected {
        let lines = matching_itemized_lines(stdout, &candidate.path);

        if lines.is_empty() || !itemized_change_needs_content_diff(candidate, &lines) {
            continue;
        }

        let destination = if itemized_change_is_new(&lines) {
            DiffSide::Empty
        } else if let Some(snapshot) = &snapshot {
            DiffSide::File(snapshot.path().join(&candidate.path))
        } else {
            DiffSide::Empty
        };
        let source = match candidate.kind {
            ChangeKind::Deleted => DiffSide::Empty,
            ChangeKind::Unchanged | ChangeKind::Changed | ChangeKind::Untracked => {
                DiffSide::File(repo_root.join(&candidate.path))
            }
        };

        diffs.insert(
            candidate.path.clone(),
            unified_content_diff(&candidate.path, destination, source)?,
        );
    }

    Ok(diffs)
}

fn paths_requiring_destination_snapshot(selected: &[Candidate], stdout: &str) -> Vec<String> {
    selected
        .iter()
        .filter(|candidate| {
            let lines = matching_itemized_lines(stdout, &candidate.path);

            itemized_change_needs_content_diff(candidate, &lines) && !itemized_change_is_new(&lines)
        })
        .map(|candidate| candidate.path.clone())
        .collect()
}

fn itemized_change_needs_content_diff(candidate: &Candidate, lines: &[&str]) -> bool {
    candidate.kind == ChangeKind::Deleted
        || lines.iter().any(|line| {
            itemized_change_is_new(&[*line])
                || line
                    .get(..11)
                    .is_some_and(|code| matches!(code.as_bytes().get(2), Some(b'c')))
                || line
                    .get(..11)
                    .is_some_and(|code| matches!(code.as_bytes().get(3), Some(b's')))
        })
}

fn itemized_change_is_new(lines: &[&str]) -> bool {
    lines
        .iter()
        .any(|line| line.get(..11).is_some_and(|code| code.contains('+')))
}

fn fetch_destination_snapshot(target: &str, paths: &[String]) -> Result<tempfile::TempDir> {
    let snapshot = tempdir().context("failed to create destination snapshot directory")?;
    let mut files_from =
        NamedTempFile::new().context("failed to create destination snapshot file list")?;

    for path in paths {
        files_from
            .write_all(path.as_bytes())
            .context("failed to write path to destination snapshot file list")?;
        files_from
            .write_all(&[0])
            .context("failed to write NUL delimiter to destination snapshot file list")?;
    }

    files_from.flush()?;

    let files_from_arg = format!("--files-from={}", files_from.path().display());
    let output = Command::new("rsync")
        .arg("-a")
        .arg("--from0")
        .arg(files_from_arg)
        .arg(target_as_source_root(target))
        .arg(snapshot.path())
        .stdin(Stdio::null())
        .output()
        .context("failed to fetch destination files for content diff")?;

    if !output.status.success() {
        bail!(
            "failed to fetch destination files for content diff:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(snapshot)
}

fn target_as_source_root(target: &str) -> String {
    if target.ends_with('/') {
        target.to_string()
    } else {
        format!("{target}/")
    }
}

enum DiffSide {
    Empty,
    File(PathBuf),
}

fn unified_content_diff(path: &str, destination: DiffSide, source: DiffSide) -> Result<String> {
    let empty = NamedTempFile::new().context("failed to create empty diff side")?;
    let destination_path = diff_side_path(&destination, empty.path());
    let source_path = diff_side_path(&source, empty.path());

    if matches!(destination, DiffSide::File(_)) && !destination_path.exists() {
        return Ok(String::from(
            "(destination file could not be fetched; content diff is unavailable)\n",
        ));
    }

    if matches!(source, DiffSide::File(_)) && !source_path.exists() {
        return Ok(String::from(
            "(source file is not available; content diff is unavailable)\n",
        ));
    }

    if is_directory(destination_path) || is_directory(source_path) {
        return Ok(String::from(
            "(content diff is not available for directories)\n",
        ));
    }

    let output = Command::new("diff")
        .arg("-u")
        .arg(destination_path)
        .arg(source_path)
        .stdin(Stdio::null())
        .output()
        .context("failed to run diff for destination content")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        return Ok(String::from("(no textual content diff)\n"));
    }

    if output.status.code() == Some(1) {
        if stdout.trim().is_empty() {
            return Ok(format!("{}\n", stderr.trim()));
        }

        return Ok(relabel_unified_diff(
            &stdout,
            &format!("destination/{path}"),
            &format!("source/{path}"),
        ));
    }

    Ok(format!("diff failed:\n{stderr}"))
}

fn diff_side_path<'a>(side: &'a DiffSide, empty_path: &'a Path) -> &'a Path {
    match side {
        DiffSide::Empty => empty_path,
        DiffSide::File(path) => path.as_path(),
    }
}

fn is_directory(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

fn relabel_unified_diff(diff: &str, old_label: &str, new_label: &str) -> String {
    let mut lines = diff.lines();
    let first = lines.next();
    let second = lines.next();

    if first.is_some_and(|line| line.starts_with("--- "))
        && second.is_some_and(|line| line.starts_with("+++ "))
    {
        let mut relabeled = format!("--- {old_label}\n+++ {new_label}\n");

        for line in lines {
            relabeled.push_str(line);
            relabeled.push('\n');
        }

        relabeled
    } else {
        diff.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use anyhow::{Context, Result};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn dry_run_uses_checksum_and_reports_same_size_content_change() -> Result<()> {
        if Command::new("rsync").arg("--version").output().is_err() {
            eprintln!("skipping test because rsync is not available");
            return Ok(());
        }

        let source_dir = tempdir()?;
        let target_dir = tempdir()?;
        let source_file = source_dir.path().join("file.txt");
        let target_file = target_dir.path().join("file.txt");

        fs::write(&source_file, "a c\n")?;
        fs::write(&target_file, "abc\n")?;

        let touch_status = Command::new("touch")
            .arg("-r")
            .arg(&source_file)
            .arg(&target_file)
            .status();

        if !touch_status.is_ok_and(|status| status.success()) {
            eprintln!("skipping test because touch -r is not available");
            return Ok(());
        }

        let candidate = Candidate {
            path: String::from("file.txt"),
            kind: ChangeKind::Changed,
            status: String::from(" M"),
        };
        let target = format!("{}/", target_dir.path().display());

        let run_report = rsync_selected(source_dir.path(), &target, &[candidate], true)
            .context("dry-run should succeed")?;
        let report = run_report.render_status(Some("file.txt"));
        let content_diff = run_report.render_content_diff(Some("file.txt"));

        assert!(report.contains("--checksum"), "{report}");
        assert!(report.contains("file.txt"), "{report}");
        assert!(
            content_diff.contains("--- destination/file.txt\n+++ source/file.txt\n"),
            "{content_diff}"
        );
        assert!(
            !report.contains("TOTAL RSYNC DIFF\n----------------\n(empty)"),
            "{report}"
        );
        assert!(
            !report.contains("CURRENT FILE RSYNC DIFF\n-----------------------\nfile.txt\n\n(no destination change for current file)"),
            "{report}"
        );

        Ok(())
    }

    #[test]
    fn report_splits_current_file_and_total_rsync_diff() {
        let candidate_a = Candidate {
            path: String::from("a.txt"),
            kind: ChangeKind::Changed,
            status: String::from(" M"),
        };
        let candidate_b = Candidate {
            path: String::from("b.txt"),
            kind: ChangeKind::Changed,
            status: String::from(" M"),
        };
        let report = RsyncRunReport {
            mode: String::from("DRY RUN"),
            repo_root: String::from("/repo"),
            target: String::from("/dest/"),
            selected: vec![candidate_a, candidate_b],
            args: vec![
                String::from("-a"),
                String::from("-i"),
                String::from("--checksum"),
            ],
            version: String::from("rsync version test"),
            delete_missing_args: true,
            stdout: String::from(">fcsT...... a.txt\n>fcsT...... b.txt\n"),
            stderr: String::new(),
            content_diffs: HashMap::from([(
                String::from("b.txt"),
                String::from("--- destination/b.txt\n+++ source/b.txt\n@@ -1 +1 @@\n-old\n+new\n"),
            )]),
        };

        let rendered = report.render_status(Some("b.txt"));
        let content_diff = report.render_content_diff(Some("b.txt"));

        assert!(rendered.contains(
            "CURRENT FILE RSYNC DIFF\n-----------------------\nb.txt\n\nItemized rsync change:\n>fcsT...... b.txt\n"
        ));
        assert!(content_diff.contains("--- destination/b.txt\n+++ source/b.txt\n"));
        assert!(rendered.contains(
            "TOTAL RSYNC DIFF\n----------------\n>fcsT...... a.txt\n>fcsT...... b.txt\n"
        ));
    }

    #[test]
    fn decodes_itemized_change() {
        let descriptions = decode_itemized_change(">fcst...... README.md");

        assert!(descriptions.contains(&String::from(
            "file data would be transferred from source to destination"
        )));
        assert!(descriptions.contains(&String::from("regular file")));
        assert!(descriptions.contains(&String::from("checksum/content differs")));
        assert!(descriptions.contains(&String::from("size differs")));
        assert!(descriptions.contains(&String::from("modification time differs")));
    }
}
