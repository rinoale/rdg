# rdg

`rdg` is a terminal UI for deploying selected files and folders from a Git
repository with `rsync`. It shows the repository tree, colors entries by Git
status, lets you select exactly what to send, previews local Git diffs, and shows
expected rsync destination changes before you deploy.

## Features

- Shows repository files and folders under the Git repository root, not only
  changed files. The `.git` and `target` directories are excluded.
- Colors files and folders by Git state: changed, untracked, deleted, or clean.
- Folders start collapsed, are expandable/collapsible, and are selectable.
  Selecting a folder includes all descendant files in the rsync preview and
  deploy.
- Auto-refreshes when files are created, updated, renamed, or deleted under the Git
  repository root.
- Preserves selected entries and expanded folders across refreshes when paths
  still exist.
- Shows a candidate list, a selected-file destination content diff pane, a Git
  diff pane, and a separate rsync destination status pane.
- Refreshes the rsync pane with a checksum dry-run when the selection changes,
  so content changes such as a one-space edit are detected against the
  destination.
- Supports deleted files when the active rsync implementation supports
  `--delete-missing-args`.
- Accepts the deploy target as an argument or through `RDG_TARGET`.

The watcher ignores `.git` and `target` so Git internals and Cargo build artifacts do
not repeatedly refresh the UI.

## Project Tree

```text
.
├── Cargo.toml
├── Cargo.lock
├── README.md
├── rust-toolchain.toml
├── scripts
│   └── install.sh     # Linux/macOS install into /usr/local/bin
└── src
    ├── app.rs          # app state, selection, refresh, rsync actions
    ├── cli.rs          # command-line parsing
    ├── core
    │   ├── git.rs      # git root/status/diff logic
    │   ├── mod.rs      # shared core helpers
    │   ├── rsync.rs    # rsync command execution and report building
    │   ├── tree.rs     # repository tree, Git status coloring, folder selection
    │   └── watcher.rs  # repository filesystem watcher
    ├── main.rs         # entrypoint
    ├── tui
    │   ├── mod.rs       # terminal setup and input/event loop
    │   ├── command.rs   # Vim-style command parsing
    │   ├── keymap.rs    # intent-based key bindings
    │   ├── menu.rs      # top hints and help text
    │   ├── style.rs     # design tokens, roles, selectors, and style DSL
    │   └── theme.rs     # named themes built from design rules
    └── ui.rs           # Ratatui rendering and preview styling
```

## Install

Install or update the stable Rust toolchain first:

```sh
rustup update stable
```

This project pins Rust `1.96.0` in `rust-toolchain.toml` and uses the 2024 edition.
Cargo is installed with Rust; with Rust `1.96.0`, Cargo is `1.96`.

Build and install into `/usr/local/bin` from this repository:

```sh
scripts/install.sh
```

The install script is compatible with Linux and macOS. It builds
`target/release/rdg`, creates `/usr/local/bin` if needed, and installs the binary
as `/usr/local/bin/rdg`. If `/usr/local/bin` is not writable, it uses `sudo`.

To install somewhere else:

```sh
INSTALL_DIR="$HOME/.local/bin" scripts/install.sh
```

To uninstall the default install:

```sh
sudo rm -f /usr/local/bin/rdg
```

## Dependencies

Build-time dependencies:

- Rust `1.96.0` or newer compatible with the pinned toolchain.
- Cargo, rustfmt, and Clippy. These are installed by `rustup` from
  `rust-toolchain.toml`.
- Rust crates from `Cargo.toml`: `anyhow`, `clap`, `crossterm`, `notify`,
  `ratatui`, and `tempfile`.

Runtime dependencies:

- `git` in `PATH`.
- `rsync` in `PATH`.
- `diff` in `PATH` for unified per-file content diffs.
- A terminal that supports the alternate screen and ANSI colors.
- For remote rsync targets such as `user@example.com:/var/www/app/`, the remote
  endpoint must also be able to run rsync. If that target uses SSH transport,
  SSH access must already be configured.

The rsync diff is produced by running rsync in dry-run mode against the actual
destination, using checksum comparison. That applies to both local destinations
and remote destinations such as `user@example.com:/var/www/app/`. Remote previews
therefore need working network/SSH access and a compatible receiver-side rsync.

When multiple files are selected, the selected-file content diff pane follows the
cursor while the rsync status pane keeps the total selected-set rsync list. Move
the cursor through the candidate list to inspect each selected file individually
without rerunning the remote dry-run.

For the current file, `rdg` shows a unified content diff between the destination
copy and the source copy when the file is textual. The rsync status pane decodes
rsync's itemized change line and shows the total selected-set rsync output. For
remote destinations, destination content is fetched into a temporary snapshot
with rsync before the content diff is rendered.

Deleted-file deploys require `--delete-missing-args`. `rdg` feature-detects this
option with `rsync --help` and only passes it when a selected deleted file needs
it. If the option is missing, `rdg` refuses the deploy before copying anything and
shows an explanation in the rsync pane. For remote targets, the receiver-side
rsync also needs to support the option; otherwise rsync itself can still reject
the run.

## Usage

Run inside a Git repository:

```sh
rdg user@example.com:/var/www/my-app/
```

Or set the target once:

```sh
export RDG_TARGET=user@example.com:/var/www/my-app/
rdg
```

Common keys:

```text
:               enter command mode
Up/Down         move through files
j/k             move through files
Space           select or unselect the current file
Enter or e      expand or collapse the current folder
Left/Right      collapse or expand the current folder
a               select all or clear all
s               show only selected files; press again for the repository tree
d               refresh the rsync destination diff
r               run rsync for selected files after the current diff is shown
Tab             switch scroll focus between Repository, content, Git, and rsync panes
Shift-Tab       switch scroll focus in reverse
F5              refresh repository candidates
?               show help overlay
Esc             cancel transient UI; it never quits
Mouse wheel     scroll the pane under the pointer; over Repository scrolls the file list
Click row       move cursor to that entry; folder rows also expand/collapse
Click checkbox  select or unselect that file or folder
Click pane      switch scroll focus to that pane
PgUp/PgDn       scroll the focused pane
Shift/Ctrl/Alt + Up/Down
                scroll the focused pane when the terminal sends modified arrows
```

Command mode:

```text
:q, :quit, :exit     quit
:help, :?            show help
:theme neutral       neutral theme
:theme safe          green safe theme
:theme danger        red danger theme
:refresh             reload repository candidates
:diff                refresh the rsync destination diff
:run, :deploy        run rsync after the current diff is shown
:select              select or unselect the current row
:all                 select all or clear all
:selected            show selected files only
:tree                show the repository tree
:focus <pane>        focus repository, content, git, or rsync
:expand              expand the current folder
:collapse            collapse the current folder
```

The TUI follows the `rustui` interface policy from `~/git/rustui`: quit is
command-mode only, bare `q` is not a global quit key, and `Ctrl-C` is not bound
to quit by default.

`rdg` also follows the `rustui` design DSL guidance. A theme is a design
stylesheet, not only a palette switch: `src/tui/style.rs` defines color tokens,
typed roles, and string selectors, while `src/tui/theme.rs` maps those selectors
to Ratatui styles. UI elements such as focused panels, footer keys, badges, Git
status rows, and diff lines use named roles/selectors instead of ad hoc colors.

## Notes

`rdg` is intentionally Git-root based. It shows the repository filesystem tree,
with Git status from `git status`. The `.git` and `target` directories are
excluded from the tree and watcher. Deleted entries come from Git status, so they
must have been tracked before deletion.

At startup, `rdg` runs `git rev-parse --show-toplevel`. If you launch it from a
directory inside a Git worktree, it watches and deploys from that repository root.
If the current directory is not inside a Git worktree, the TUI does not start and
Git's "not a git repository" error is shown.
