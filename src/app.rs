use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::deleter::{self, DeleteResult};
use crate::types::{
    AppStats, ColorPalette, DeleteAnimation, DeletePhase, FolderStatus, FoundFolder, RiskLevel,
    ScanConfig, ScanProgress, SortDirection, SortField, Tab, Theme,
};

const SPINNER_CHARS: &[char] = &['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];

pub struct App {
    pub config: ScanConfig,
    pub folders: Vec<FoundFolder>,
    pub filtered_indices: Vec<usize>,
    pub list_selected: Option<usize>,
    pub scroll_offset: usize,
    pub stats: AppStats,
    pub scan_start: Option<Instant>,
    pub status_message: String,
    pub should_quit: bool,

    // Tabs
    pub active_tab: Tab,

    // Search
    pub search_mode: bool,
    pub search_query: String,

    // Sort
    pub sort_field: SortField,
    pub sort_direction: SortDirection,

    // Animations
    pub frame_count: u64,
    pub delete_animations: Vec<DeleteAnimation>,

    // ETA / progress
    pub total_dirs_visited: u64,
    pub eta_seconds: Option<f64>,
    pub scan_progress: f64,
    pub scan_complete: bool,
    pub current_scan_path: String,

    // Theme
    pub theme: Theme,

    // Settings cursor
    pub settings_cursor: usize,
}

impl App {
    pub fn new(config: ScanConfig) -> Self {
        Self {
            config,
            folders: Vec::new(),
            filtered_indices: Vec::new(),
            list_selected: None,
            scroll_offset: 0,
            stats: AppStats::default(),
            scan_start: None,
            status_message: "Scanning...".to_string(),
            should_quit: false,
            active_tab: Tab::Scan,
            search_mode: false,
            search_query: String::new(),
            sort_field: SortField::Size,
            sort_direction: SortDirection::Desc,
            frame_count: 0,
            delete_animations: Vec::new(),
            total_dirs_visited: 0,
            eta_seconds: None,
            scan_progress: 0.0,
            scan_complete: false,
            current_scan_path: String::new(),
            theme: Theme::Catppuccino,
            settings_cursor: 0,
        }
    }

    pub fn palette(&self) -> ColorPalette {
        self.theme.palette()
    }

    pub fn start_scan(&mut self) {
        self.scan_start = Some(Instant::now());
        self.status_message = "Scanning...".to_string();
        self.scan_complete = false;
    }

    pub fn process_scan_results(&mut self, results: Vec<FoundFolder>) {
        self.folders = results;
        self.stats.total_found = self.folders.len();
        self.stats.total_size_reclaimable = deleter::get_total_size(&self.folders);
        self.scan_complete = true;
        if let Some(start) = self.scan_start {
            self.stats.scan_duration_secs = start.elapsed().as_secs_f64();
        }
        let seconds = self.stats.scan_duration_secs;
        self.status_message = format!(
            "Scan completed in {seconds:.1}s — {} folder(s) found ({})",
            self.folders.len(),
            deleter::format_size(self.stats.total_size_reclaimable),
        );
        self.apply_sort();
    }

    pub fn real_index(&self, display_idx: usize) -> Option<usize> {
        self.filtered_indices.get(display_idx).copied()
    }

    pub fn selected_real_index(&self) -> Option<usize> {
        self.list_selected
            .and_then(|d| self.filtered_indices.get(d).copied())
    }

    pub fn apply_sort(&mut self) {
        let field = self.sort_field;
        let dir = self.sort_direction;
        self.folders.sort_by(|a, b| {
            let cmp = match field {
                SortField::Size => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
                SortField::Date => a
                    .last_modified
                    .unwrap_or(0)
                    .cmp(&b.last_modified.unwrap_or(0)),
                SortField::Path => a
                    .path
                    .display()
                    .to_string()
                    .cmp(&b.path.display().to_string()),
            };
            match dir {
                SortDirection::Asc => cmp,
                SortDirection::Desc => cmp.reverse(),
            }
        });
        self.rebuild_filter();
    }

    pub fn rebuild_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        if q.is_empty() {
            self.filtered_indices = (0..self.folders.len()).collect();
        } else {
            self.filtered_indices = self
                .folders
                .iter()
                .enumerate()
                .filter(|(_, f)| f.path.to_string_lossy().to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect();
        }
        let max = self.filtered_indices.len().saturating_sub(1);
        if let Some(sel) = self.list_selected {
            if sel > max {
                self.list_selected = if max > 0 { Some(max) } else { None };
            }
        } else if !self.filtered_indices.is_empty() {
            self.list_selected = Some(0);
        }
    }

    pub fn next(&mut self, count: usize) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let cur = self.list_selected.unwrap_or(0);
        self.list_selected = Some((cur + count).min(len - 1));
        self.clamp_scroll();
    }

    pub fn previous(&mut self, count: usize) {
        let cur = self.list_selected.unwrap_or(0);
        self.list_selected = Some(cur.saturating_sub(count));
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        if let Some(sel) = self.list_selected {
            if sel < self.scroll_offset {
                self.scroll_offset = sel;
            }
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(real_i) = self.selected_real_index() {
            if real_i >= self.folders.len() {
                return;
            }
            let folder = &mut self.folders[real_i];
            if folder.status == FolderStatus::Deleted || folder.status == FolderStatus::Deleting {
                return;
            }
            folder.status = FolderStatus::Deleting;
            let result = deleter::delete_folder(folder, self.config.dry_run);
            match result {
                DeleteResult::Success | DeleteResult::DryRun => {
                    folder.status = FolderStatus::Deleted;
                    self.stats.total_deleted += 1;
                    if let Some(s) = folder.size {
                        self.stats.total_size_freed += s;
                    }
                    self.delete_animations.push(DeleteAnimation {
                        folder_index: real_i,
                        started_at: Instant::now(),
                        phase: DeletePhase::Deleting,
                    });
                }
                DeleteResult::Failed => {
                    folder.status = FolderStatus::Error;
                    self.stats.total_errors += 1;
                }
            }
        }
    }

    pub fn delete_all_folders(&mut self) {
        let dry = self.config.dry_run;
        let indices: Vec<usize> = self
            .filtered_indices
            .iter()
            .copied()
            .filter(|&i| {
                let s = self.folders[i].status;
                s != FolderStatus::Deleted && s != FolderStatus::Deleting
            })
            .collect();
        for real_i in indices {
            if real_i >= self.folders.len() {
                continue;
            }
            let folder = &mut self.folders[real_i];
            folder.status = FolderStatus::Deleting;
            let result = deleter::delete_folder(folder, dry);
            match result {
                DeleteResult::Success | DeleteResult::DryRun => {
                    folder.status = FolderStatus::Deleted;
                    self.stats.total_deleted += 1;
                    if let Some(s) = folder.size {
                        self.stats.total_size_freed += s;
                    }
                }
                DeleteResult::Failed => {
                    folder.status = FolderStatus::Error;
                    self.stats.total_errors += 1;
                }
            }
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort_field = match self.sort_field {
            SortField::Size => SortField::Date,
            SortField::Date => SortField::Path,
            SortField::Path => SortField::Size,
        };
        self.apply_sort();
    }

    pub fn toggle_sort_direction(&mut self) {
        self.sort_direction = self.sort_direction.flip();
        self.apply_sort();
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.rebuild_filter();
    }

    pub fn update_animations(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
        self.delete_animations.retain(|anim| {
            let elapsed = anim.started_at.elapsed().as_millis();
            match anim.phase {
                DeletePhase::Deleting => elapsed < 300,
                DeletePhase::Fading => elapsed < 800,
                DeletePhase::Done => false,
            }
        });
    }

    pub fn scan_progress_bar(&self) -> String {
        let pct = (self.scan_progress * 100.0).min(100.0) as u8;
        let filled = (pct as usize).saturating_div(10);
        let empty = 10usize.saturating_sub(filled);
        format!("[{}{}] {}%", "█".repeat(filled), "░".repeat(empty), pct)
    }

    pub fn spinner_char(&self) -> char {
        SPINNER_CHARS[(self.frame_count as usize) % SPINNER_CHARS.len()]
    }

    pub fn sort_indicator(&self) -> String {
        let arrow = match self.sort_direction {
            SortDirection::Asc => "▲",
            SortDirection::Desc => "▼",
        };
        let field = match self.sort_field {
            SortField::Size => "Size",
            SortField::Date => "Date",
            SortField::Path => "Path",
        };
        format!("{arrow} {field}")
    }
}

// ── Entry point ──────────────────────────────────────────────

pub fn run_tui(app: &mut Arc<Mutex<App>>, progress: Arc<Mutex<ScanProgress>>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

    let res = run(&mut terminal, app, progress);

    let mut stdout = io::stdout();
    let _ = stdout.execute(crossterm::event::DisableMouseCapture);
    disable_raw_mode()?;
    let _ = terminal.backend_mut().execute(LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    res
}

fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: &mut Arc<Mutex<App>>,
    progress: Arc<Mutex<ScanProgress>>,
) -> io::Result<()> {
    let mut last_render = Instant::now();
    let min_frame_time = std::time::Duration::from_millis(33);

    loop {
        {
            let mut a = app.lock().unwrap();
            a.update_animations();
            if let Ok(p) = progress.lock() {
                a.current_scan_path = p.current_path.clone();
                a.stats.total_found = a.stats.total_found.max(p.folders_found);
                a.stats.total_size_reclaimable =
                    a.stats.total_size_reclaimable.max(p.total_size_reclaimable);
            }
        }

        let now = Instant::now();
        if now - last_render >= min_frame_time {
            terminal.draw(|f| ui(f, app))?;
            last_render = now;
        }

        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let mut a = app.lock().unwrap();

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if a.search_mode {
                            a.search_mode = false;
                        } else if a.active_tab != Tab::Scan {
                            a.active_tab = Tab::Scan;
                        } else {
                            a.should_quit = true;
                        }
                        continue;
                    }
                    KeyCode::Char('1') => a.active_tab = Tab::Scan,
                    KeyCode::Char('2') => a.active_tab = Tab::Settings,
                    KeyCode::Char('3') => a.active_tab = Tab::Help,
                    KeyCode::Tab => {
                        a.active_tab = match a.active_tab {
                            Tab::Scan => Tab::Settings,
                            Tab::Settings => Tab::Help,
                            Tab::Help => Tab::Scan,
                        };
                    }
                    _ => {}
                }

                match a.active_tab {
                    Tab::Settings => handle_settings_key(&mut a, key.code),
                    Tab::Help => {}
                    Tab::Scan => handle_scan_key(&mut a, key.code),
                }

                // Force immediate render on input
                last_render = Instant::now()
                    .checked_sub(min_frame_time)
                    .unwrap_or(Instant::now());
            }

            if let Event::Mouse(mouse) = event::read()? {
                let mut a = app.lock().unwrap();
                match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        if a.active_tab == Tab::Scan {
                            if let Ok(size) = terminal.size() {
                                let row = mouse.row as usize;
                                let tab_h = 1;
                                let list_start = tab_h + 4;
                                let list_end = (size.height as usize).saturating_sub(1);
                                if row >= list_start && row < list_end {
                                    let display_idx = row - list_start + a.scroll_offset;
                                    if display_idx < a.filtered_indices.len() {
                                        a.list_selected = Some(display_idx);
                                        a.clamp_scroll();
                                    }
                                }
                            }
                        }
                    }
                    MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                        if a.active_tab == Tab::Scan {
                            a.delete_selected();
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if a.active_tab == Tab::Scan {
                            a.next(1);
                        }
                    }
                    MouseEventKind::ScrollUp if a.active_tab == Tab::Scan => {
                        a.previous(1);
                    }
                    _ => {}
                }

                last_render = Instant::now()
                    .checked_sub(min_frame_time)
                    .unwrap_or(Instant::now());
            }
        }

        if app.lock().unwrap().should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_scan_key(a: &mut App, code: KeyCode) {
    if a.search_mode {
        match code {
            KeyCode::Esc | KeyCode::Enter => a.search_mode = false,
            KeyCode::Backspace => {
                let mut q = a.search_query.clone();
                q.pop();
                a.set_search_query(q);
            }
            KeyCode::Char(c) => {
                let mut q = a.search_query.clone();
                q.push(c);
                a.set_search_query(q);
            }
            _ => {}
        }
        return;
    }

    match code {
        KeyCode::Char('h') | KeyCode::Char('?') => a.active_tab = Tab::Help,
        KeyCode::Char('j') | KeyCode::Down => a.next(1),
        KeyCode::Char('k') | KeyCode::Up => a.previous(1),
        KeyCode::PageDown => a.next(10),
        KeyCode::PageUp => a.previous(10),
        KeyCode::Home | KeyCode::Char('g') => {
            if !a.filtered_indices.is_empty() {
                a.list_selected = Some(0);
                a.scroll_offset = 0;
            }
        }
        KeyCode::End | KeyCode::Char('G') => {
            let len = a.filtered_indices.len();
            if len > 0 {
                a.list_selected = Some(len - 1);
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => a.delete_selected(),
        KeyCode::Char('d') => {
            if a.config.delete_all {
                a.delete_all_folders();
            }
        }
        KeyCode::Char('s') => a.cycle_sort(),
        KeyCode::Char('S') => a.toggle_sort_direction(),
        KeyCode::Char('/') => {
            a.search_mode = true;
            a.search_query.clear();
            a.rebuild_filter();
        }
        _ => {}
    }
}

fn handle_settings_key(a: &mut App, code: KeyCode) {
    let settings_len = 6;
    match code {
        KeyCode::Char('j') | KeyCode::Down => {
            a.settings_cursor = (a.settings_cursor + 1).min(settings_len - 1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            a.settings_cursor = a.settings_cursor.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Char(' ') => match a.settings_cursor {
            0 => {
                // Theme cycle
                let themes = Theme::all();
                let idx = themes.iter().position(|t| *t == a.theme).unwrap_or(0);
                a.theme = themes[(idx + 1) % themes.len()];
            }
            1 => a.config.dry_run = !a.config.dry_run,
            2 => a.config.exclude_sensitive = !a.config.exclude_sensitive,
            3 => a.config.disable_size = !a.config.disable_size,
            4 => a.config.disable_age = !a.config.disable_age,
            5 => {
                a.sort_field = match a.sort_field {
                    SortField::Size => SortField::Date,
                    SortField::Date => SortField::Path,
                    SortField::Path => SortField::Size,
                }
            }
            _ => {}
        },
        _ => {}
    }
}

// ── UI ───────────────────────────────────────────────────────

fn ui(frame: &mut Frame, app: &mut Arc<Mutex<App>>) {
    let a = app.lock().unwrap();
    let p = a.palette();
    let _bg = Style::default().bg(p.bg).fg(p.fg);

    let vert = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]);
    let chunks = vert.split(frame.area());

    draw_tab_bar(frame, chunks[0], &a, &p);
    draw_content(frame, chunks[1], &a, &p);
    draw_status_bar(frame, chunks[2], &a, &p);
}

// ── Tab bar ──────────────────────────────────────────────────

fn draw_tab_bar(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let tabs = [" 1 Scan ", " 2 Settings ", " 3 Help "];
    let mut spans = Vec::new();
    for (i, label) in tabs.iter().enumerate() {
        let tab = match i {
            0 => Tab::Scan,
            1 => Tab::Settings,
            _ => Tab::Help,
        };
        let is_active = app.active_tab == tab;
        let style = if is_active {
            Style::default()
                .bg(p.accent)
                .fg(p.highlight_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(p.surface).fg(p.dim)
        };
        spans.push(Span::styled(*label, style));
        spans.push(Span::raw(" "));
    }
    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().bg(p.bg));
    frame.render_widget(para, area);
}

// ── Content ──────────────────────────────────────────────────

fn draw_content(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    match app.active_tab {
        Tab::Scan => draw_scan_tab(frame, area, app, p),
        Tab::Settings => draw_settings_tab(frame, area, app, p),
        Tab::Help => draw_help_tab(frame, area, app, p),
    }
}

// ── Scan tab ─────────────────────────────────────────────────

fn draw_scan_tab(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let sections = Layout::vertical([
        Constraint::Length(8), // header box (bordered)
        Constraint::Min(0),    // folder list box (bordered)
    ]);
    let chunks = sections.split(area);
    if chunks.len() < 2 {
        return;
    }

    draw_scan_header_box(frame, chunks[0], app, p);
    draw_folder_box(frame, chunks[1], app, p);
}

fn draw_scan_header_box(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Scan Summary ")
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 5 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1), // title + sort + search
        Constraint::Length(1), // stats row 1
        Constraint::Length(1), // stats row 2 (time + reclaimable)
        Constraint::Length(1), // scan path
        Constraint::Length(1), // progress bar
    ]);
    let chunks = rows.split(inner);

    // Row 0: app title + sort indicator + search hint
    let title = format!(" npkill-rs v{}", env!("CARGO_PKG_VERSION"));
    let sort_text = app.sort_indicator();
    let search_hint = if app.search_mode {
        format!("  / {} _", app.search_query)
    } else {
        "  / search".to_string()
    };

    let title_line = Line::from(vec![
        Span::styled(
            &title,
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(&sort_text, Style::default().fg(p.success)),
        Span::styled(&search_hint, Style::default().fg(p.dim)),
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    // Row 1: stats — found / deleted / errors
    let stats_line = format!(
        " Found: {}   Deleted: {}   Errors: {}",
        app.stats.total_found, app.stats.total_deleted, app.stats.total_errors,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &stats_line,
            Style::default().fg(p.fg),
        ))),
        chunks[1],
    );

    // Row 2: reclaimable + scan time
    let time_str = if app.scan_complete {
        let s = app.stats.scan_duration_secs;
        format!("{:.2}s", s)
    } else if app.stats.total_found > 0 {
        "scanning...".to_string()
    } else {
        "--".to_string()
    };
    let time_line = format!(
        " Reclaimable: {}   Scan time: {}",
        deleter::format_size(app.stats.total_size_reclaimable),
        time_str,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &time_line,
            Style::default().fg(p.fg),
        ))),
        chunks[2],
    );

    // Row 3: current scan path
    let scan_path = if !app.current_scan_path.is_empty() {
        format!(" Path: {}", app.current_scan_path)
    } else if !app.scan_complete {
        " Path: scanning...".to_string()
    } else {
        String::new()
    };
    let path_style = if !app.scan_complete {
        Style::default().fg(p.warning)
    } else {
        Style::default().fg(p.success)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(&scan_path, path_style))),
        chunks[3],
    );

    // Row 4: progress bar (only during scan)
    if !app.scan_complete {
        let spinner = if !app.folders.is_empty() {
            format!("{} ", app.spinner_char())
        } else {
            String::new()
        };
        let eta_str = app
            .eta_seconds
            .filter(|&e| e > 0.0)
            .map_or_else(String::new, |eta| format!(" ETA {:.0}s", eta));
        let progress_str = app.scan_progress_bar();
        let status = format!(" {}{}{}", spinner, progress_str, eta_str);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                &status,
                Style::default().fg(p.accent),
            ))),
            chunks[4],
        );
    }
}

fn draw_folder_box(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let title = if app.search_query.is_empty() {
        format!(" Folders ({}) ", app.filtered_indices.len())
    } else {
        format!(
            " Folders ({}/{} filtered) ",
            app.filtered_indices.len(),
            app.folders.len()
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));

    if app.filtered_indices.is_empty() {
        let msg = if app.search_query.is_empty() {
            " No folders found. Scanning..."
        } else {
            " No folders match your search."
        };
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(p.dim)))),
            inner,
        );
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Inner layout: column header + list
    let sections = Layout::vertical([
        Constraint::Length(1), // column header
        Constraint::Min(0),    // folder list
    ]);
    let chunks = sections.split(inner);
    if chunks.len() < 2 {
        return;
    }

    draw_column_headers(frame, chunks[0], app, p);

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .map(|&ri| {
            let f = &app.folders[ri];
            let tag = match f.target {
                crate::types::TargetKind::NodeModules => "NM",
                crate::types::TargetKind::NextDotNext => "NX",
            };
            let sc = match f.status {
                FolderStatus::Pending => " ",
                FolderStatus::Deleting => ">",
                FolderStatus::Deleted => "D",
                FolderStatus::Error => "E",
            };
            let rm = if f.risk == RiskLevel::Sensitive {
                "!"
            } else {
                " "
            };
            let sz = f
                .size
                .map(deleter::format_size)
                .unwrap_or_else(|| "?".into());
            let age = f.last_modified.map_or_else(String::new, |ts| {
                format!(" {:>3}d", (now_secs - ts) / 86400)
            });
            let line = format!(" {sc} [{tag}]{rm}  {sz:>8}{age}  {}", f.path.display());

            let st = match f.status {
                FolderStatus::Deleted => Style::default().fg(p.dim).crossed_out(),
                FolderStatus::Error => Style::default().fg(p.error),
                _ if f.risk == RiskLevel::Sensitive => Style::default().fg(p.warning),
                _ => Style::default().fg(p.fg),
            };
            ListItem::new(line).style(st)
        })
        .collect();

    let list_w = List::new(items)
        .highlight_style(
            Style::default()
                .fg(p.highlight_fg)
                .bg(p.highlight_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸");

    let mut ls = ListState::default();
    ls.select(app.list_selected);
    // List handles scroll automatically based on selected item
    frame.render_stateful_widget(list_w, chunks[1], &mut ls);
}

fn draw_column_headers(frame: &mut Frame, area: Rect, _app: &App, p: &ColorPalette) {
    let header = Line::from(vec![
        Span::styled(
            " S [Tag]",
            Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "Size",
            Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::raw("     "),
        Span::styled(
            "Age",
            Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "Path",
            Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .style(Style::default().bg(p.bg))
        .borders(Borders::TOP)
        .border_type(ratatui::widgets::BorderType::Plain)
        .border_style(Style::default().fg(p.surface));
    frame.render_widget(
        Paragraph::new(header)
            .block(block)
            .style(Style::default().bg(p.bg)),
        area,
    );
}

// ── Settings tab ─────────────────────────────────────────────

fn draw_settings_tab(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let items: Vec<ListItem> = vec![
        format!("Theme: {}", app.theme.name()),
        format!("Dry run: {}", if app.config.dry_run { "ON" } else { "off" }),
        format!(
            "Exclude sensitive: {}",
            if app.config.exclude_sensitive {
                "ON"
            } else {
                "off"
            }
        ),
        format!(
            "Disable size: {}",
            if app.config.disable_size { "ON" } else { "off" }
        ),
        format!(
            "Disable age: {}",
            if app.config.disable_age { "ON" } else { "off" }
        ),
        format!(
            "Sort: {} {}",
            app.sort_indicator(),
            match app.sort_direction {
                SortDirection::Asc => "↑",
                SortDirection::Desc => "↓",
            }
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, text)| {
        let is_cursor = i == app.settings_cursor;
        let prefix = if is_cursor { "▸ " } else { "  " };
        let st = if is_cursor {
            Style::default()
                .fg(p.highlight_fg)
                .bg(p.highlight_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg).bg(p.bg)
        };
        ListItem::new(format!("{}{}", prefix, text)).style(st)
    })
    .collect();

    let list_w = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Settings ")
            .style(Style::default().bg(p.bg).fg(p.dim)),
    );

    let mut ls = ListState::default();
    ls.select(Some(app.settings_cursor));
    frame.render_stateful_widget(list_w, area, &mut ls);

    // Draw hint text below settings
    let bottom = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]);
    let chunks = bottom.split(area);
    if chunks.len() > 1 {
        let hint = Paragraph::new(Line::from(Span::styled(
            " ↑/j ↓/k navigate  Enter/Space toggle  Esc/Settings back",
            Style::default().fg(p.dim).bg(p.bg),
        )))
        .style(Style::default().bg(p.bg));
        frame.render_widget(hint, chunks[1]);
    }
}

// ── Help tab ─────────────────────────────────────────────────

fn draw_help_tab(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    // Use app to get current keybindings context
    let _ = app;
    let lines = vec![
        Line::from(Span::styled(
            "Help",
            Style::default()
                .fg(p.accent)
                .add_modifier(Modifier::BOLD)
                .bg(p.bg),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Navigation",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  ↑/k / ↓/j     Move selection")),
        Line::from(Span::raw("  PgUp / PgDn   Page scroll")),
        Line::from(Span::raw("  Home/g / End/G First / last")),
        Line::from(Span::raw("  Mouse click    Select")),
        Line::from(Span::raw("  Scroll wheel   Navigate")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Actions",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  Enter / Space  Delete selected")),
        Line::from(Span::raw("  Right click    Delete selected")),
        Line::from(Span::raw("  d              Delete all (--delete-all)")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Sorting & Search",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  s              Cycle sort (Size/Date/Path)")),
        Line::from(Span::raw("  S              Reverse sort direction")),
        Line::from(Span::raw("  /              Enter search mode")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Tabs",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  1 / 2 / 3      Switch tab")),
        Line::from(Span::raw("  Tab            Cycle tabs")),
        Line::from(Span::raw("  Esc            Back to Scan tab")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Other",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  q / Esc        Quit")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " CLI Options",
            Style::default().fg(p.success).bg(p.bg),
        )),
        Line::from(Span::raw("  -d <PATH>      Root directory")),
        Line::from(Span::raw("  -b <DIR>       Blacklist")),
        Line::from(Span::raw("  -w <DIR>       Whitelist")),
        Line::from(Span::raw("  --dry-run      Simulate deletions")),
        Line::from(Span::raw("  --delete-all   Auto delete")),
        Line::from(Span::raw("  --exclude-sensitive")),
    ];

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(p.bg).fg(p.dim)),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(p.bg));
    frame.render_widget(para, area);
}

// ── Status bar ───────────────────────────────────────────────

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let left = match app.active_tab {
        Tab::Scan => " ↑↓/j/k nav  Enter del  / search  s sort  ? help  q quit",
        Tab::Settings => " ↑/j ↓/k nav  Enter toggle  Esc back",
        Tab::Help => " Esc back to Scan",
    };

    let mut right = String::new();
    if app.config.dry_run {
        right.push_str(" DRY RUN ");
    }
    if !app.scan_complete {
        right.push_str(" SCANNING ");
    }

    let left_spans = Span::styled(left, Style::default().fg(p.dim).bg(p.bg));
    let right_spans = Span::styled(
        &right,
        Style::default()
            .fg(p.warning)
            .bg(p.bg)
            .add_modifier(Modifier::BOLD),
    );

    let block = Block::default()
        .title(Line::from(left_spans))
        .title_bottom(Line::from(right_spans))
        .style(Style::default().bg(p.bg));
    frame.render_widget(block, area);
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FolderStatus, TargetKind};
    use std::path::PathBuf;

    fn make_folder(path: &str, size: u64, status: FolderStatus) -> FoundFolder {
        FoundFolder {
            path: PathBuf::from(path),
            target: TargetKind::NodeModules,
            size: Some(size),
            last_modified: None,
            status,
            risk: RiskLevel::Safe,
            risk_reason: None,
        }
    }

    fn setup_app() -> App {
        let config = ScanConfig::default();
        let mut app = App::new(config);
        app.folders = vec![
            make_folder("/a", 100, FolderStatus::Pending),
            make_folder("/b", 200, FolderStatus::Pending),
            make_folder("/c", 50, FolderStatus::Pending),
        ];
        app.apply_sort();
        app
    }

    #[test]
    fn test_app_new() {
        let app = App::new(ScanConfig::default());
        assert!(app.folders.is_empty());
        assert!(!app.should_quit);
        assert_eq!(app.active_tab, Tab::Scan);
    }

    #[test]
    fn test_apply_sort_size_desc() {
        let app = setup_app();
        assert_eq!(app.folders[0].size, Some(200));
        assert_eq!(app.folders[2].size, Some(50));
    }

    #[test]
    fn test_cycle_sort() {
        let mut app = setup_app();
        assert_eq!(app.sort_field, SortField::Size);
        app.cycle_sort();
        assert_eq!(app.sort_field, SortField::Date);
        app.cycle_sort();
        assert_eq!(app.sort_field, SortField::Path);
    }

    #[test]
    fn test_toggle_sort_direction() {
        let mut app = setup_app();
        app.toggle_sort_direction();
        assert_eq!(app.folders[0].size, Some(50));
    }

    #[test]
    fn test_rebuild_filter() {
        let mut app = setup_app();
        assert_eq!(app.filtered_indices.len(), 3);
        app.set_search_query("/a".to_string());
        assert_eq!(app.filtered_indices.len(), 1);
        app.set_search_query("".to_string());
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn test_delete_selected() {
        let mut app = App::new(ScanConfig {
            dry_run: true,
            ..Default::default()
        });
        app.folders = vec![make_folder("/tmp/t", 1024, FolderStatus::Pending)];
        app.apply_sort();
        app.delete_selected();
        assert_eq!(app.stats.total_deleted, 1);
        assert_eq!(app.stats.total_size_freed, 1024);
    }

    #[test]
    fn test_next_previous() {
        let mut app = setup_app();
        app.next(1);
        assert_eq!(app.list_selected, Some(1));
        app.previous(1);
        assert_eq!(app.list_selected, Some(0));
    }

    #[test]
    fn test_settings_cursor() {
        let mut app = setup_app();
        app.active_tab = Tab::Settings;
        handle_settings_key(&mut app, KeyCode::Char('j'));
        assert_eq!(app.settings_cursor, 1);
        handle_settings_key(&mut app, KeyCode::Char('k'));
        assert_eq!(app.settings_cursor, 0);
    }

    #[test]
    fn test_tab_switching() {
        let mut app = setup_app();
        assert_eq!(app.active_tab, Tab::Scan);
        app.active_tab = Tab::Settings;
        assert_eq!(app.active_tab, Tab::Settings);
    }

    #[test]
    fn test_process_scan_results() {
        let mut app = App::new(ScanConfig::default());
        let folder = make_folder("/tmp/t", 512, FolderStatus::Pending);
        app.process_scan_results(vec![folder]);
        assert_eq!(app.stats.total_found, 1);
        assert!(app.scan_complete);
    }

    #[test]
    fn test_theme_default() {
        let app = App::new(ScanConfig::default());
        assert_eq!(app.theme, Theme::Catppuccino);
    }

    #[test]
    fn test_theme_cycle() {
        let mut app = setup_app();
        app.active_tab = Tab::Settings;
        assert_eq!(app.theme, Theme::Catppuccino);
        // Toggle theme via Enter on cursor 0
        app.settings_cursor = 0;
        handle_settings_key(&mut app, KeyCode::Enter);
        assert_eq!(app.theme, Theme::Nord);
        handle_settings_key(&mut app, KeyCode::Enter);
        assert_eq!(app.theme, Theme::TokyoNight);
        handle_settings_key(&mut app, KeyCode::Enter);
        assert_eq!(app.theme, Theme::Catppuccino);
    }

    #[test]
    fn test_theme_palette_not_empty() {
        let p = Theme::Catppuccino.palette();
        let _ = format!("{:?}", p.bg);
    }

    #[test]
    fn test_scan_progress_bar() {
        let mut app = setup_app();
        app.scan_progress = 0.5;
        let bar = app.scan_progress_bar();
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));
        assert!(bar.contains("50%"));
    }

    #[test]
    fn test_spinner_char() {
        let mut app = setup_app();
        app.frame_count = 0;
        let c = app.spinner_char();
        assert!(SPINNER_CHARS.contains(&c));
    }

    #[test]
    fn test_sort_indicator() {
        let app = setup_app();
        let ind = app.sort_indicator();
        assert!(ind.contains("Size"));
        assert!(ind.contains('▼'));
    }
}
