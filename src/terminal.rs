use std::{
    io::{self, Stdout},
    sync::mpsc::Receiver,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    app::{App, PreviewPane},
    core::watcher::{self, WatchEvent},
    ui,
};

type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn run(mut app: App) -> Result<()> {
    let (_watcher, watcher_rx) = watcher::watch_repo(&app.repo_root)?;
    let mut terminal = setup_terminal()?;
    let app_result = run_app(&mut terminal, &mut app, watcher_rx);
    let restore_result = restore_terminal(&mut terminal);

    app_result?;
    restore_result?;

    Ok(())
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    Ok(terminal)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run_app(terminal: &mut Tui, app: &mut App, watcher_rx: Receiver<WatchEvent>) -> Result<()> {
    loop {
        refresh_after_fs_events(app, &watcher_rx);
        terminal.draw(|frame| ui::draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.should_quit = true;
                    }
                    KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                        move_repository_cursor(terminal, app, true)?;
                    }
                    KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                        move_repository_cursor(terminal, app, false)?;
                    }
                    KeyCode::Char('j') => {
                        move_repository_cursor(terminal, app, true)?;
                    }
                    KeyCode::Char('k') => {
                        move_repository_cursor(terminal, app, false)?;
                    }
                    KeyCode::Enter | KeyCode::Char('e') => {
                        app.toggle_current_expanded();
                    }
                    KeyCode::Right => {
                        app.expand_current();
                    }
                    KeyCode::Left => {
                        app.collapse_current();
                    }
                    KeyCode::Down | KeyCode::PageDown => {
                        scroll_active_pane(terminal, app, true)?;
                    }
                    KeyCode::Up | KeyCode::PageUp => {
                        scroll_active_pane(terminal, app, false)?;
                    }
                    KeyCode::Char(' ') => {
                        app.toggle_current();
                    }
                    KeyCode::Char('a') => {
                        app.toggle_all();
                    }
                    KeyCode::Char('d') => {
                        app.refresh_rsync_preview(true);
                    }
                    KeyCode::Char('r') => {
                        app.deploy_rsync();
                    }
                    KeyCode::Tab => {
                        app.toggle_active_pane();
                    }
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => {
                    handle_mouse_scroll(terminal, app, mouse.column, mouse.row, true)?
                }
                MouseEventKind::ScrollUp => {
                    handle_mouse_scroll(terminal, app, mouse.column, mouse.row, false)?
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    handle_mouse_click(terminal, app, mouse.column, mouse.row)?;
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}

fn handle_mouse_click(terminal: &Tui, app: &mut App, column: u16, row: u16) -> Result<()> {
    let areas = ui::body_areas(terminal.size()?.into());

    if contains(areas.candidates, column, row) {
        app.set_active_pane(PreviewPane::Repository);
        let viewport_rows = usize::from(areas.candidates.height.saturating_sub(2));
        let view_offset = app.repository_view_offset(viewport_rows);

        if let Some((index, toggles_selection)) = candidate_hit(
            areas.candidates,
            column,
            row,
            app.visible_indices().len(),
            view_offset,
        ) {
            if toggles_selection {
                app.toggle_candidate_at(index);
            } else {
                app.set_cursor(index);
                app.toggle_current_expanded();
            }
        }
    } else if contains(areas.content, column, row) {
        app.set_active_pane(PreviewPane::Content);
    } else if contains(areas.git, column, row) {
        app.set_active_pane(PreviewPane::Git);
    } else if contains(areas.rsync, column, row) {
        app.set_active_pane(PreviewPane::Rsync);
    }

    Ok(())
}

fn handle_mouse_scroll(
    terminal: &Tui,
    app: &mut App,
    column: u16,
    row: u16,
    down: bool,
) -> Result<()> {
    let areas = ui::body_areas(terminal.size()?.into());

    if contains(areas.candidates, column, row) {
        app.set_active_pane(PreviewPane::Repository);
    } else if contains(areas.content, column, row) {
        app.set_active_pane(PreviewPane::Content);
    } else if contains(areas.git, column, row) {
        app.set_active_pane(PreviewPane::Git);
    } else if contains(areas.rsync, column, row) {
        app.set_active_pane(PreviewPane::Rsync);
    }

    scroll_active_pane(terminal, app, down)?;

    Ok(())
}

fn move_repository_cursor(terminal: &Tui, app: &mut App, down: bool) -> Result<()> {
    if down {
        app.move_down();
    } else {
        app.move_up();
    }

    app.ensure_repository_cursor_visible(repository_viewport_rows(terminal)?);
    Ok(())
}

fn scroll_active_pane(terminal: &Tui, app: &mut App, down: bool) -> Result<()> {
    match app.active_pane {
        PreviewPane::Repository => {
            let viewport_rows = repository_viewport_rows(terminal)?;

            if down {
                app.scroll_repository_down(viewport_rows);
            } else {
                app.scroll_repository_up(viewport_rows);
            }
        }
        PreviewPane::Content | PreviewPane::Git | PreviewPane::Rsync => {
            if down {
                app.scroll_preview_down();
            } else {
                app.scroll_preview_up();
            }
        }
    }

    Ok(())
}

fn repository_viewport_rows(terminal: &Tui) -> Result<usize> {
    Ok(usize::from(
        ui::body_areas(terminal.size()?.into())
            .candidates
            .height
            .saturating_sub(2),
    ))
}

fn candidate_hit(
    area: ratatui::layout::Rect,
    column: u16,
    row: u16,
    len: usize,
    view_offset: usize,
) -> Option<(usize, bool)> {
    let inner_x = area.x.saturating_add(1);
    let inner_y = area.y.saturating_add(1);
    let inner_bottom = area.y.saturating_add(area.height).saturating_sub(1);

    if row < inner_y || row >= inner_bottom {
        return None;
    }

    let index = view_offset.saturating_add(usize::from(row - inner_y));

    if index >= len {
        return None;
    }

    let toggles_selection = column >= inner_x && column <= inner_x.saturating_add(6);

    Some((index, toggles_selection))
}

fn contains(area: ratatui::layout::Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn refresh_after_fs_events(app: &mut App, watcher_rx: &Receiver<WatchEvent>) {
    let mut refresh = false;

    for event in watcher_rx.try_iter() {
        match event {
            Ok(event) if watcher::should_refresh(&app.repo_root, &event) => {
                refresh = true;
            }
            Ok(_) => {}
            Err(err) => {
                app.set_message(format!("File watcher error: {err}"));
            }
        }
    }

    if refresh && let Err(err) = app.refresh_candidates() {
        app.set_message(format!("Auto-refresh failed: {err:#}"));
    }
}
