use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
};
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
    pub active_tab: Tab,
    pub search_mode: bool,
    pub search_query: String,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub frame_count: u64,
    pub delete_animations: Vec<DeleteAnimation>,
    pub total_dirs_visited: u64,
    pub total_dirs_estimate: u64,
    pub scan_progress: f64,
    pub scan_complete: bool,
    pub current_scan_path: String,
    pub theme: Theme,
    pub settings_cursor: usize,
    /// True when a popup (settings/help) is open over the scan tab.
    pub popup_open: bool,
    pub update_available: Option<String>,
    pub delete_start: Option<Instant>,
    pub pending_delete_count: usize,
    pub delete_eta_seconds: Option<f64>,
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
            scan_start: Some(Instant::now()),
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
            total_dirs_estimate: 0,
            scan_progress: 0.0,
            scan_complete: false,
            current_scan_path: String::new(),
            theme: Theme::Catppuccino,
            settings_cursor: 0,
            popup_open: false,
            update_available: None,
            delete_start: None,
            pending_delete_count: 0,
            delete_eta_seconds: None,
        }
    }

    pub fn palette(&self) -> ColorPalette {
        self.theme.palette()
    }

    pub fn process_scan_results(&mut self, results: Vec<FoundFolder>) {
        for r in results {
            if let Some(existing) = self.folders.iter_mut().find(|f| f.path == r.path) {
                existing.size = r.size;
                existing.last_modified = r.last_modified;
                existing.status = r.status;
                existing.risk = r.risk;
                existing.risk_reason = r.risk_reason;
            } else {
                self.folders.push(r);
            }
        }
        self.stats.total_found = self.folders.len();
        self.stats.total_size_reclaimable = deleter::get_total_size(&self.folders);
        self.scan_complete = true;
        self.scan_progress = 1.0;
        if let Some(start) = self.scan_start {
            self.stats.scan_duration_secs = start.elapsed().as_secs_f64();
        }
        let seconds = self.stats.scan_duration_secs;
        self.status_message = format!(
            "Scan completed in {:.1}s — {} folder(s) found ({})",
            seconds,
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
                    self.update_delete_eta();
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
                    self.update_delete_eta();
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
                DeletePhase::Deleting => elapsed < 500,
                DeletePhase::Fading => elapsed < 1200,
                DeletePhase::Done => false,
            }
        });
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

    fn format_duration(secs: f64) -> String {
        if secs < 0.001 {
            "<1ms".to_string()
        } else if secs < 1.0 {
            format!("{:.0}ms", secs * 1000.0)
        } else if secs < 60.0 {
            format!("{:.1}s", secs)
        } else {
            let m = (secs as u64) / 60;
            let s = (secs as u64) % 60;
            format!("{}m {:02}s", m, s)
        }
    }

    fn update_delete_eta(&mut self) {
        let now = Instant::now();
        if self.delete_start.is_none() {
            self.delete_start = Some(now);
        }
        let elapsed = self.delete_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.01).max(0.01);
        let rate = self.stats.total_deleted as f64 / elapsed;
        let remaining = self.pending_delete_count.saturating_sub(self.stats.total_deleted);
        self.delete_eta_seconds = if rate > 0.0 && remaining > 0 {
            Some(remaining as f64 / rate)
        } else {
            None
        };
    }

    fn delete_progress(&self, real_idx: usize) -> f64 {
        for anim in &self.delete_animations {
            if anim.folder_index == real_idx {
                let elapsed = anim.started_at.elapsed().as_millis() as f64;
                return match anim.phase {
                    DeletePhase::Deleting => (elapsed / 500.0).min(1.0),
                    DeletePhase::Fading => 1.0,
                    DeletePhase::Done => 1.0,
                };
            }
        }
        0.0
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

    let mut last_sort_count: usize = 0;

    loop {
        {
            let mut a = app.lock().unwrap();
            a.update_animations();
            if let Ok(mut p) = progress.lock() {
                a.current_scan_path = p.current_path.clone();
                a.total_dirs_visited = p.dirs_visited;
                a.total_dirs_estimate = p.total_dirs_estimate;
                a.stats.total_found = a.stats.total_found.max(p.folders_found);
                a.stats.total_size_reclaimable =
                    a.stats.total_size_reclaimable.max(p.total_size_reclaimable);

                // Drain pending folders from scanner
                for f in p.pending_folders.drain(..) {
                    if !a.folders.iter().any(|existing| existing.path == f.path) {
                        a.folders.push(f);
                    }
                }
                if a.folders.len() != last_sort_count && !a.folders.is_empty() {
                    last_sort_count = a.folders.len();
                    a.apply_sort();
                }

                // Count pending deletions for ETA
                if a.scan_complete {
                    a.pending_delete_count = a.folders.iter().filter(|f| f.status == FolderStatus::Pending).count();
                }

                if !a.scan_complete {
                    let total = p.total_dirs_estimate.max(1);
                    a.scan_progress = (p.dirs_visited as f64 / total as f64).min(0.99);
                }
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
                    KeyCode::Esc => {
                        if a.popup_open {
                            a.popup_open = false;
                            a.active_tab = Tab::Scan;
                            continue;
                        }
                        if a.search_mode {
                            a.search_mode = false;
                        } else if a.active_tab != Tab::Scan {
                            a.active_tab = Tab::Scan;
                        } else {
                            a.should_quit = true;
                        }
                        continue;
                    }
                    KeyCode::Char('q') => {
                        if !a.popup_open && !a.search_mode {
                            a.should_quit = true;
                        }
                        continue;
                    }
                    KeyCode::Char('2') => {
                        a.popup_open = true;
                        a.active_tab = Tab::Settings;
                    }
                    KeyCode::Char('3') | KeyCode::Char('?') | KeyCode::Char('h') => {
                        a.popup_open = true;
                        a.active_tab = Tab::Help;
                    }
                    KeyCode::Tab => {
                        if a.popup_open {
                            a.popup_open = false;
                            a.active_tab = Tab::Scan;
                        } else {
                            a.popup_open = true;
                            a.active_tab = Tab::Settings;
                        }
                    }
                    _ => {}
                }

                if a.popup_open {
                    match a.active_tab {
                        Tab::Settings => handle_settings_key(&mut a, key.code),
                        Tab::Help => {}
                        _ => {}
                    }
                } else if a.active_tab == Tab::Scan {
                    handle_scan_key(&mut a, key.code);
                }

                last_render = Instant::now()
                    .checked_sub(min_frame_time)
                    .unwrap_or(Instant::now());
            }

                if let Event::Mouse(mouse) = event::read()? {
                let mut a = app.lock().unwrap();
                if a.popup_open {
                    // Modal: all mouse events are swallowed while popup is open
                } else if a.active_tab == Tab::Scan {
                    match mouse.kind {
                        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
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
                        MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                            a.delete_selected();
                        }
                        MouseEventKind::ScrollDown => a.next(1),
                        MouseEventKind::ScrollUp => a.previous(1),
                        _ => {}
                    }
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

    let vert = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ]);
    let chunks = vert.split(frame.area());

    draw_tab_bar(frame, chunks[0], &a, &p);
    // Always draw scan tab content as base layer
    draw_scan_tab(frame, chunks[1], &a, &p);
    // Overlay popup if open
    if a.popup_open {
        draw_popup(frame, frame.area(), &a, &p);
    }
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
        let is_active = if i > 0 && app.popup_open {
            app.active_tab == tab
        } else {
            !app.popup_open && app.active_tab == tab
        };
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
    frame.render_widget(Paragraph::new(line).style(Style::default().bg(p.bg)), area);
}

// ── Scan tab ─────────────────────────────────────────────────

fn draw_scan_tab(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let gap = 1;
    let total = area.height;
    let header_h = 9.min(total.saturating_sub(2).max(3));
    let sections = Layout::vertical([
        Constraint::Length(header_h),
        Constraint::Length(gap),
        Constraint::Min(0),
    ]);
    let chunks = sections.split(area);
    if chunks.len() < 3 {
        return;
    }

    draw_scan_header_box(frame, chunks[0], app, p);
    draw_folder_box(frame, chunks[2], app, p);
}

fn draw_scan_header_box(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" npkill-rs v{} ", env!("CARGO_PKG_VERSION")))
        .title_alignment(Alignment::Left)
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 6 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ]);
    let chunks = rows.split(inner);

    let sort_text = app.sort_indicator();
    let search_hint = if app.search_mode {
        format!("  / {} ▌", app.search_query)
    } else {
        "  / search".to_string()
    };
    let title_line = Line::from(vec![
        Span::styled(" npkill-rs ", Style::default().fg(p.accent).add_modifier(Modifier::BOLD)),
        Span::styled("|", Style::default().fg(p.dim)),
        Span::raw(" "),
        Span::styled(&sort_text, Style::default().fg(p.success).add_modifier(Modifier::BOLD)),
        Span::styled(&search_hint, Style::default().fg(p.dim)),
    ]);
    frame.render_widget(Paragraph::new(title_line), chunks[0]);

    let found_str = format!("{}", app.stats.total_found);
    let deleted_str = format!("{}", app.stats.total_deleted);
    let errors_str = format!("{}", app.stats.total_errors);
    let reclaimable_str = deleter::format_size(app.stats.total_size_reclaimable);
    let freed_str = deleter::format_size(app.stats.total_size_freed);

    let stats1 = Line::from(vec![
        Span::styled(" Found:  ", Style::default().fg(p.dim)),
        Span::styled(&found_str, Style::default().fg(p.accent).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("│", Style::default().fg(p.surface)),
        Span::raw("  "),
        Span::styled("Deleted: ", Style::default().fg(p.dim)),
        Span::styled(&deleted_str, Style::default().fg(p.success)),
        Span::raw("  "),
        Span::styled("│", Style::default().fg(p.surface)),
        Span::raw("  "),
        Span::styled("Errors: ", Style::default().fg(p.dim)),
        Span::styled(&errors_str, Style::default().fg(p.error)),
    ]);
    frame.render_widget(Paragraph::new(stats1), chunks[1]);

    let time_elapsed = app.scan_start.map(|s| App::format_duration(s.elapsed().as_secs_f64())).unwrap_or_default();
    let time_str = if app.scan_complete {
        App::format_duration(app.stats.scan_duration_secs)
    } else {
        time_elapsed
    };
    let delete_eta_str = app
        .delete_eta_seconds
        .filter(|&e| e > 0.0 && e.is_finite())
        .map(App::format_duration)
        .unwrap_or_default();
    let stats2 = if !delete_eta_str.is_empty() {
        Line::from(vec![
            Span::styled(" Reclaimable: ", Style::default().fg(p.dim)),
            Span::styled(&reclaimable_str, Style::default().fg(p.warning).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("│", Style::default().fg(p.surface)),
            Span::raw("  "),
            Span::styled("Delete ETA: ", Style::default().fg(p.dim)),
            Span::styled(&delete_eta_str, Style::default().fg(p.accent).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("│", Style::default().fg(p.surface)),
            Span::raw("  "),
            Span::styled("Freed: ", Style::default().fg(p.dim)),
            Span::styled(&freed_str, Style::default().fg(p.success)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Reclaimable: ", Style::default().fg(p.dim)),
            Span::styled(&reclaimable_str, Style::default().fg(p.warning).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("│", Style::default().fg(p.surface)),
            Span::raw("  "),
            Span::styled("Elapsed: ", Style::default().fg(p.dim)),
            Span::styled(time_str, Style::default().fg(p.accent)),
            Span::raw("  "),
            Span::styled("│", Style::default().fg(p.surface)),
            Span::raw("  "),
            Span::styled("Freed: ", Style::default().fg(p.dim)),
            Span::styled(&freed_str, Style::default().fg(p.success)),
        ])
    };
    frame.render_widget(Paragraph::new(stats2), chunks[2]);

    let scan_path = if !app.current_scan_path.is_empty() {
        &app.current_scan_path
    } else if !app.scan_complete {
        "scanning..."
    } else {
        ""
    };
    let path_style = if !app.scan_complete {
        Style::default().fg(p.warning)
    } else {
        Style::default().fg(p.success)
    };
    let path_line = Line::from(vec![
        Span::styled(" Path: ", Style::default().fg(p.dim)),
        Span::styled(scan_path, path_style),
    ]);
    frame.render_widget(Paragraph::new(path_line), chunks[3]);

    // Row 4: progress
    if !app.scan_complete {
        let spinner = app.spinner_char();
        let dirs = app.total_dirs_visited;
        let elapsed = app.scan_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
        let elapsed_str = App::format_duration(elapsed);
        let progress_str = format!(" {}  {} elapsed  {}/{} dirs  found: {}", spinner, elapsed_str, dirs, app.total_dirs_estimate.max(dirs), app.folders.len());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(&progress_str, Style::default().fg(p.accent)))),
            chunks[4],
        );
    } else {
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(p.success).bg(p.surface).add_modifier(Modifier::BOLD))
            .percent(100)
            .label(format!(
                " ✓ {} — {} folders, {}",
                App::format_duration(app.stats.scan_duration_secs),
                app.folders.len(),
                deleter::format_size(app.stats.total_size_reclaimable),
            ));
        frame.render_widget(gauge, chunks[4]);
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
        .border_type(BorderType::Rounded)
        .title(title)
        .title_alignment(Alignment::Left)
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));

    if app.filtered_indices.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let msg = if app.search_query.is_empty() {
            if app.scan_complete {
                " No folders found."
            } else {
                " Searching..."
            }
        } else {
            " No folders match your search."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(p.dim)))),
            inner,
        );
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
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
        .enumerate()
        .map(|(display_i, &ri)| {
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

            // Stylish deletion: show progress bar and gradually erase path
            let is_deleting = f.status == FolderStatus::Deleting || f.status == FolderStatus::Deleted;
            let (line, line_style) = if is_deleting {
                let progress = app.delete_progress(ri);
                if f.status == FolderStatus::Deleted && progress >= 1.0 {
                    let bar = progress_bar(progress, 6);
                    let msg = format!("   {}  Deleted  {}", bar, tag);
                    (msg, Style::default().fg(p.dim))
                } else if f.status == FolderStatus::Deleting || progress < 1.0 {
                    let bar = progress_bar(progress, 6);
                    let sz = f.size.map(deleter::format_size).unwrap_or_else(|| "?".into());
                    let full_path = f.path.display().to_string();
                    let visible_chars = ((1.0 - progress) * full_path.len() as f64).max(3.0) as usize;
                    let erased_path: String = full_path.chars().take(visible_chars).collect();
                    let age = f.last_modified.map_or_else(String::new, |ts| {
                        format!(" {:>3}d", (now_secs - ts) / 86400)
                    });
                    let line = format!(" > [{tag}] {bar} {sz:>8}{age}  {erased_path}");
                    (line, Style::default().fg(p.warning))
                } else {
                    let bar = progress_bar(1.0, 6);
                    let msg = format!("   {}  Done  {}", bar, tag);
                    (msg, Style::default().fg(p.dim))
                }
            } else {
                let sz = f.size.map(deleter::format_size).unwrap_or_else(|| "?".into());
                let age = f.last_modified.map_or_else(String::new, |ts| {
                    format!(" {:>3}d", (now_secs - ts) / 86400)
                });
                let line = format!(" {sc} [{tag}]{rm}  {sz:>8}{age}  {}", f.path.display());
                let is_even = display_i % 2 == 0;
                let row_bg = if is_even { p.bg } else { p.surface };
                let st = match f.status {
                    FolderStatus::Deleted => Style::default().fg(p.dim).bg(row_bg).crossed_out(),
                    FolderStatus::Error => Style::default().fg(p.error).bg(row_bg),
                    _ if f.risk == RiskLevel::Sensitive => Style::default().fg(p.warning).bg(row_bg),
                    _ => Style::default().fg(p.fg).bg(row_bg),
                };
                (line, st)
            };

            ListItem::new(line).style(line_style)
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
    frame.render_stateful_widget(list_w, chunks[1], &mut ls);
}

fn progress_bar(progress: f64, width: usize) -> String {
    let filled = (progress * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn draw_column_headers(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let sort_field = app.sort_field;
    let col_status = Span::styled(" S  ", Style::default().fg(p.dim));
    let col_tag = Span::styled("[Tag]", Style::default().fg(p.dim));

    let (size_style, date_style, path_style) = match sort_field {
        SortField::Size => (
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            Style::default().fg(p.dim),
            Style::default().fg(p.dim),
        ),
        SortField::Date => (
            Style::default().fg(p.dim),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            Style::default().fg(p.dim),
        ),
        SortField::Path => (
            Style::default().fg(p.dim),
            Style::default().fg(p.dim),
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
    };

    let header = Line::from(vec![
        col_status,
        col_tag,
        Span::raw(" "),
        Span::styled("Size", size_style),
        Span::raw("    "),
        Span::styled(" Age ", date_style),
        Span::styled(" Path", path_style),
    ]);
    let block = Block::default()
        .style(Style::default().bg(p.bg))
        .borders(Borders::TOP)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(p.surface));
    frame.render_widget(
        Paragraph::new(header).block(block).style(Style::default().bg(p.bg)),
        area,
    );
}

// ── Popup (overlay for Settings / Help) ──────────────────────

fn draw_popup(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    // Dimmed backdrop
    frame.render_widget(Clear, area);

    let popup_width = area.width.clamp(40, 60);
    let popup_height = match app.active_tab {
        Tab::Settings => area.height.clamp(12, 16),
        Tab::Help => area.height.clamp(18, 24),
        _ => 12,
    };
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    match app.active_tab {
        Tab::Settings => draw_settings_popup(frame, popup_area, app, p),
        Tab::Help => draw_help_popup(frame, popup_area, app, p),
        _ => {}
    }
}

fn draw_settings_popup(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Settings ")
        .title_alignment(Alignment::Left)
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = vec![
        format!("Theme: {}", app.theme.name()),
        format!("Dry run: {}", if app.config.dry_run { "ON" } else { "off" }),
        format!("Exclude sensitive: {}", if app.config.exclude_sensitive { "ON" } else { "off" }),
        format!("Disable size: {}", if app.config.disable_size { "ON" } else { "off" }),
        format!("Disable age: {}", if app.config.disable_age { "ON" } else { "off" }),
        format!("Sort: {} {}", app.sort_indicator(), match app.sort_direction { SortDirection::Asc => "↑", SortDirection::Desc => "↓" }),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, text)| {
        let is_cursor = i == app.settings_cursor;
        let prefix = if is_cursor { "▸ " } else { "  " };
        let is_even = i % 2 == 0;
        let row_bg = if is_even { p.bg } else { p.surface };
        let st = if is_cursor {
            Style::default().fg(p.highlight_fg).bg(p.highlight_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.fg).bg(row_bg)
        };
        ListItem::new(format!("{}{}", prefix, text)).style(st)
    })
    .collect();

    let list_w = List::new(items);
    let mut ls = ListState::default();
    ls.select(Some(app.settings_cursor));
    frame.render_stateful_widget(list_w, inner, &mut ls);

    let hint = Paragraph::new(Line::from(Span::styled(
        " ↑/j ↓/k  Enter toggle  Esc close",
        Style::default().fg(p.dim),
    )))
    .style(Style::default().bg(p.bg));
    let hint_area = Rect::new(area.x, area.y + area.height.saturating_sub(2), area.width, 1);
    frame.render_widget(hint, hint_area);
}

fn draw_help_popup(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let _ = app;
    let lines = vec![
        Line::from(Span::styled("Help", Style::default().fg(p.accent).add_modifier(Modifier::BOLD))),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Navigation", Style::default().fg(p.success))),
        Line::from(Span::raw("  ↑/k ↓/j     Move selection       PgUp/PgDn  Page")),
        Line::from(Span::raw("  Home/g End/G  First / last")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Actions", Style::default().fg(p.success))),
        Line::from(Span::raw("  Enter/Space  Delete selected")),
        Line::from(Span::raw("  d            Delete all (--delete-all)")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Sort & Search", Style::default().fg(p.success))),
        Line::from(Span::raw("  s            Cycle sort        S  Reverse sort")),
        Line::from(Span::raw("  /            Search")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" Other", Style::default().fg(p.success))),
        Line::from(Span::raw("  2 / Tab     Settings    3 / ? / h  Help")),
        Line::from(Span::raw("  q / Esc     Quit / close")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" CLI Options", Style::default().fg(p.success))),
        Line::from(Span::raw("  --json         JSON output    --table    Table output")),
        Line::from(Span::raw("  --dry-run      Simulate       --delete-all  Auto del")),
        Line::from(Span::raw("  -m / --max-depth  Limit search depth")),
        Line::from(Span::raw("  --exclude-sensitive")),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Help ")
        .title_alignment(Alignment::Left)
        .title_style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(p.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(p.bg));
    frame.render_widget(para, inner);

    let hint = Paragraph::new(Line::from(Span::styled(
        " Esc close",
        Style::default().fg(p.dim),
    )))
    .style(Style::default().bg(p.bg));
    let hint_area = Rect::new(area.x, area.y + area.height.saturating_sub(2), area.width, 1);
    frame.render_widget(hint, hint_area);
}

// ── Status bar ───────────────────────────────────────────────

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App, p: &ColorPalette) {
    let left = if app.popup_open {
        match app.active_tab {
            Tab::Settings => " ↑/j ↓/k  Enter toggle  Esc close",
            Tab::Help => " Esc close",
            _ => " Esc close",
        }
    } else {
        match app.active_tab {
            Tab::Scan => " ↑↓/j/k nav  Enter del  / search  s sort  2 settings  3 help  q quit",
            _ => " Esc back",
        }
    };

    let mut right_parts: Vec<Span> = Vec::new();
    if app.search_mode {
        right_parts.push(Span::styled(
            " SEARCH ",
            Style::default().fg(p.highlight_fg).bg(p.accent).add_modifier(Modifier::BOLD),
        ));
        right_parts.push(Span::raw(" "));
    }
    if app.config.dry_run {
        right_parts.push(Span::styled(
            " DRY RUN ",
            Style::default().fg(p.bg).bg(p.warning).add_modifier(Modifier::BOLD),
        ));
        right_parts.push(Span::raw(" "));
    }
    if !app.scan_complete {
        right_parts.push(Span::styled(
            " SCANNING ",
            Style::default().fg(p.bg).bg(p.accent).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(ref ver) = app.update_available {
        right_parts.push(Span::styled(
            format!(" UPDATE {ver} "),
            Style::default().fg(p.bg).bg(p.warning).add_modifier(Modifier::BOLD),
        ));
    }

    let block = Block::default()
        .title(Line::from(Span::styled(left, Style::default().fg(p.dim))))
        .title_bottom(Line::from(right_parts))
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
        let mut app = App::new(ScanConfig { dry_run: true, ..Default::default() });
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

    #[test]
    fn test_format_duration() {
        assert_eq!(App::format_duration(0.0005), "<1ms");
        assert_eq!(App::format_duration(0.05), "50ms");
        assert_eq!(App::format_duration(1.5), "1.5s");
        assert_eq!(App::format_duration(125.0), "2m 05s");
    }
}
