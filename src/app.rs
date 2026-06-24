use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

use crate::deleter::{self, DeleteResult};
use crate::types::{
    AppStats, DeleteAnimation, DeletePhase, FolderStatus, FoundFolder, RiskLevel, ScanConfig,
    SortDirection, SortField,
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
    pub show_help: bool,
    pub should_quit: bool,
    pub search_mode: bool,
    pub search_query: String,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub frame_count: u64,
    pub delete_animations: Vec<DeleteAnimation>,
    pub total_dirs_visited: u64,
    pub eta_seconds: Option<f64>,
    pub scan_progress: f64,
    pub scan_complete: bool,
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
            show_help: false,
            should_quit: false,
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
        }
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
                SortField::Date => a.last_modified.unwrap_or(0).cmp(&b.last_modified.unwrap_or(0)),
                SortField::Path => {
                    a.path.display().to_string().cmp(&b.path.display().to_string())
                }
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

    pub fn page_down(&mut self, page_size: usize) {
        self.next(page_size);
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.previous(page_size);
    }

    pub fn go_top(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.list_selected = Some(0);
            self.scroll_offset = 0;
        }
    }

    pub fn go_bottom(&mut self) {
        let len = self.filtered_indices.len();
        if len > 0 {
            self.list_selected = Some(len - 1);
        }
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
            if folder.status == FolderStatus::Deleted
                || folder.status == FolderStatus::Deleting
            {
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
            SortDirection::Asc => "▴",
            SortDirection::Desc => "▾",
        };
        let field = match self.sort_field {
            SortField::Size => "Size",
            SortField::Date => "Date",
            SortField::Path => "Path",
        };
        format!("{arrow} {field}")
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
}

pub fn run_tui(app: &mut Arc<Mutex<App>>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

    let res = run(&mut terminal, app);

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
) -> io::Result<()> {
    loop {
        {
            let mut a = app.lock().unwrap();
            a.update_animations();
        }

        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let mut a = app.lock().unwrap();
                    if a.search_mode {
                        match key.code {
                            KeyCode::Esc | KeyCode::Enter => {
                                a.search_mode = false;
                            }
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
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                if a.show_help {
                                    a.show_help = false;
                                } else {
                                    a.should_quit = true;
                                }
                            }
                            KeyCode::Char('h') | KeyCode::Char('?') => {
                                a.toggle_help();
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                a.next(1);
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                a.previous(1);
                            }
                            KeyCode::PageDown => {
                                a.page_down(10);
                            }
                            KeyCode::PageUp => {
                                a.page_up(10);
                            }
                            KeyCode::Home | KeyCode::Char('g') => {
                                a.go_top();
                            }
                            KeyCode::End | KeyCode::Char('G') => {
                                a.go_bottom();
                            }
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                a.delete_selected();
                            }
                            KeyCode::Char('d') => {
                                if a.config.delete_all {
                                    a.delete_all_folders();
                                }
                            }
                            KeyCode::Char('s') => {
                                a.cycle_sort();
                            }
                            KeyCode::Char('S') => {
                                a.toggle_sort_direction();
                            }
                            KeyCode::Char('/') => {
                                a.search_mode = true;
                                a.search_query.clear();
                                a.rebuild_filter();
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Event::Mouse(mouse) = event::read()? {
                let mut a = app.lock().unwrap();
                match mouse.kind {
                    MouseEventKind::Down(btn)
                        if btn == crossterm::event::MouseButton::Left =>
                    {
                        let size = terminal.size().ok();
                        if let Some(size) = size {
                            let row = mouse.row as usize;
                            let list_start = 3usize;
                            let list_end = (size.height as usize).saturating_sub(1);
                            if row >= list_start && row < list_end {
                                let display_idx =
                                    row - list_start + a.scroll_offset;
                                if display_idx < a.filtered_indices.len() {
                                    a.list_selected = Some(display_idx);
                                    a.clamp_scroll();
                                }
                            }
                            if row == 0 && mouse.column >= 25 && mouse.column <= 40 {
                                a.cycle_sort();
                            }
                        }
                    }
                    MouseEventKind::Down(btn)
                        if btn == crossterm::event::MouseButton::Right =>
                    {
                        a.delete_selected();
                    }
                    MouseEventKind::ScrollDown => {
                        a.next(1);
                    }
                    MouseEventKind::ScrollUp => {
                        a.previous(1);
                    }
                    _ => {}
                }
            }
        }

        if app.lock().unwrap().should_quit {
            break;
        }
    }

    Ok(())
}

fn ui(frame: &mut Frame, app: &mut Arc<Mutex<App>>) {
    let a = app.lock().unwrap();

    if a.show_help {
        draw_help(frame);
        return;
    }

    let bg = Style::default().bg(Color::Black);

    let areas = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    draw_header(frame, areas[0], &a, bg);
    draw_folder_list(frame, areas[1], &a, bg);
    draw_footer(frame, areas[2], &a, bg);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App, bg: Style) {
    if area.height < 4 {
        return;
    }
    let vert =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]);
    let rows = vert.split(area);

    let title = format!(" npkill-rs v{}", env!("CARGO_PKG_VERSION"));
    let sort_text = app.sort_indicator();
    let search_hint = if app.search_mode {
        format!(" / {} ", app.search_query)
    } else {
        " /  search".to_string()
    };

    let title_line = Line::from(vec![
        Span::styled(
            &title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .bg(Color::Black),
        ),
        Span::raw("  "),
        Span::styled(&sort_text, Style::default().fg(Color::Green).bg(Color::Black)),
        Span::raw("  "),
        Span::styled(
            &search_hint,
            Style::default().fg(Color::DarkGray).bg(Color::Black),
        ),
    ]);
    frame.render_widget(Paragraph::new(title_line).style(bg), rows[0]);

    let stats_line = format!(
        " Found: {}  Deleted: {}  Errors: {}  Reclaimable: {}  Freed: {}",
        app.stats.total_found,
        app.stats.total_deleted,
        app.stats.total_errors,
        deleter::format_size(app.stats.total_size_reclaimable),
        deleter::format_size(app.stats.total_size_freed),
    );
    let stats_para = Paragraph::new(Line::from(Span::raw(&stats_line)))
        .style(Style::default().fg(Color::White).bg(Color::Black));
    frame.render_widget(stats_para, rows[1]);

    let spinner = if !app.scan_complete && !app.folders.is_empty() {
        format!("{} ", app.spinner_char())
    } else {
        String::new()
    };
    let eta_str = match app.eta_seconds {
        Some(eta) if eta > 0.0 => format!(" ETA {:.0}s", eta),
        _ => String::new(),
    };
    let progress_str = if !app.scan_complete {
        format!(" {}", app.scan_progress_bar())
    } else {
        String::new()
    };
    let status = format!("{}{}{} {}", spinner, app.status_message, eta_str, progress_str);
    let status_para = Paragraph::new(Line::from(Span::styled(
        &status,
        Style::default().fg(Color::Yellow).bg(Color::Black),
    )))
    .style(bg);
    frame.render_widget(status_para, rows[2]);

    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(area.width.saturating_sub(1).max(0) as usize),
        Style::default().fg(Color::DarkGray).bg(Color::Black),
    )))
    .style(bg);
    frame.render_widget(sep, rows[3]);
}

fn draw_folder_list(frame: &mut Frame, area: Rect, app: &App, bg: Style) {
    if app.filtered_indices.is_empty() {
        let msg = if app.search_query.is_empty() {
            "No folders found. Scanning..."
        } else {
            "No folders match your search."
        };
        let para = Paragraph::new(Line::from(Span::raw(msg)))
            .style(Style::default().fg(Color::DarkGray).bg(Color::Black))
            .block(Block::default().style(bg));
        frame.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(_display_idx, &real_idx)| {
            let f = &app.folders[real_idx];
            let target_tag = match f.target {
                crate::types::TargetKind::NodeModules => "NM",
                crate::types::TargetKind::NextDotNext => "NX",
            };
            let status_char = match f.status {
                FolderStatus::Pending => " ",
                FolderStatus::Deleting => ">",
                FolderStatus::Deleted => "D",
                FolderStatus::Error => "E",
            };
            let risk_mark = if f.risk == RiskLevel::Sensitive {
                "!"
            } else {
                " "
            };
            let size_str = f
                .size
                .map(deleter::format_size)
                .unwrap_or_else(|| "?".to_string());
            let age_str = f.last_modified.map_or_else(
                || "".to_string(),
                |ts| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    let days = (now - ts) / 86400;
                    format!(" {:3}d", days)
                },
            );
            let line = format!(
                " {status_char} [{target_tag}]{risk_mark} {size_str:>10}{age_str}  {}",
                f.path.display()
            );
            let style = match f.status {
                FolderStatus::Deleted => {
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Black)
                        .crossed_out()
                }
                FolderStatus::Error => Style::default().fg(Color::Red).bg(Color::Black),
                _ if f.risk == RiskLevel::Sensitive => {
                    Style::default().fg(Color::Yellow).bg(Color::Black)
                }
                _ => Style::default().fg(Color::White).bg(Color::Black),
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let title = if app.search_query.is_empty() {
        format!(" Folders ({}) ", app.filtered_indices.len())
    } else {
        format!(
            " Folders ({}/{} filtered) ",
            app.filtered_indices.len(),
            app.folders.len()
        )
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title).style(bg))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸");

    let mut list_state = ListState::default();
    list_state.select(app.list_selected);

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App, bg: Style) {
    let spans = vec![
        Span::styled(" ↑↓/j/k ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("nav  "),
        Span::styled(" Enter ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("del  "),
        Span::styled(" / ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("search  "),
        Span::styled(" s ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("sort  "),
        Span::styled(" h ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("help  "),
        Span::styled(" q ", Style::default().fg(Color::Cyan).bg(Color::Black)),
        Span::raw("quit"),
    ];

    let mut right_tags = String::new();
    if app.config.dry_run {
        right_tags.push_str(" DRY RUN ");
    }
    if !app.scan_complete {
        right_tags.push_str(" SCANNING ");
    }

    let line = Line::from(spans);
    let block = Block::default()
        .title(line)
        .title_bottom(Line::from(Span::styled(
            &right_tags,
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )))
        .style(bg);

    frame.render_widget(block, area);
}

fn draw_help(frame: &mut Frame) {
    let bg = Style::default().bg(Color::Black);
    let help_text = vec![
        Line::from(Span::styled(
            "npkill-rs Help",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .bg(Color::Black),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Navigation",
            Style::default().fg(Color::Green).bg(Color::Black),
        )),
        Line::from(Span::raw("  ↑ / k  — Move selection up")),
        Line::from(Span::raw("  ↓ / j  — Move selection down")),
        Line::from(Span::raw("  PgUp/PgDn — Page up/down")),
        Line::from(Span::raw("  Home/g / End/G — First / last")),
        Line::from(Span::raw("  Mouse click — Select item")),
        Line::from(Span::raw("  Scroll wheel — Navigate")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Actions",
            Style::default().fg(Color::Green).bg(Color::Black),
        )),
        Line::from(Span::raw("  Enter/Space/Right-click — Delete selected")),
        Line::from(Span::raw("  d — Delete all (with --delete-all)")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Sorting & Search",
            Style::default().fg(Color::Green).bg(Color::Black),
        )),
        Line::from(Span::raw("  s — Cycle sort (Size → Date → Path)")),
        Line::from(Span::raw("  S — Reverse sort direction")),
        Line::from(Span::raw("  / — Enter search mode")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " Other",
            Style::default().fg(Color::Green).bg(Color::Black),
        )),
        Line::from(Span::raw("  q / Esc / Ctrl-C — Quit")),
        Line::from(Span::raw("  h / ? — Toggle this help")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            " CLI Options",
            Style::default().fg(Color::Green).bg(Color::Black),
        )),
        Line::from(Span::raw("  -d, --directory <PATH>  Root directory")),
        Line::from(Span::raw("  -b, --blacklist <DIR>  Always skip")),
        Line::from(Span::raw("  -w, --whitelist <DIR>  Never skip")),
        Line::from(Span::raw("      --dry-run         Simulate")),
        Line::from(Span::raw("      --exclude-sensitive")),
        Line::from(Span::raw("      --delete-all")),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(bg),
        )
        .style(bg);

    frame.render_widget(paragraph, frame.area());
}

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

    fn setup_filtered_app() -> App {
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
        let config = ScanConfig::default();
        let app = App::new(config);
        assert!(app.folders.is_empty());
        assert_eq!(app.stats.total_deleted, 0);
        assert!(!app.should_quit);
        assert!(!app.search_mode);
    }

    #[test]
    fn test_apply_sort_size_desc() {
        let app = setup_filtered_app();
        assert_eq!(app.sort_field, SortField::Size);
        assert_eq!(app.sort_direction, SortDirection::Desc);
        assert_eq!(app.folders[0].size, Some(200));
        assert_eq!(app.folders[2].size, Some(50));
    }

    #[test]
    fn test_cycle_sort() {
        let mut app = setup_filtered_app();
        assert_eq!(app.sort_field, SortField::Size);
        app.cycle_sort();
        assert_eq!(app.sort_field, SortField::Date);
        app.cycle_sort();
        assert_eq!(app.sort_field, SortField::Path);
        app.cycle_sort();
        assert_eq!(app.sort_field, SortField::Size);
    }

    #[test]
    fn test_toggle_sort_direction() {
        let mut app = setup_filtered_app();
        assert_eq!(app.sort_direction, SortDirection::Desc);
        app.toggle_sort_direction();
        assert_eq!(app.sort_direction, SortDirection::Asc);
        assert_eq!(app.folders[0].size, Some(50));
    }

    #[test]
    fn test_rebuild_filter() {
        let mut app = setup_filtered_app();
        assert_eq!(app.filtered_indices.len(), 3);
        app.set_search_query("/a".to_string());
        assert_eq!(app.filtered_indices.len(), 1);
        app.set_search_query("".to_string());
        assert_eq!(app.filtered_indices.len(), 3);
    }

    #[test]
    fn test_delete_selected_updates_stats() {
        let mut app = App::new(ScanConfig {
            dry_run: true,
            ..Default::default()
        });
        app.folders = vec![make_folder("/tmp/test_nm", 1024, FolderStatus::Pending)];
        app.apply_sort();
        app.delete_selected();
        assert_eq!(app.stats.total_deleted, 1);
        assert_eq!(app.stats.total_size_freed, 1024);
        assert_eq!(app.folders[0].status, FolderStatus::Deleted);
    }

    #[test]
    fn test_next_and_previous() {
        let mut app = setup_filtered_app();
        app.next(1);
        assert_eq!(app.list_selected, Some(1));
        app.next(1);
        assert_eq!(app.list_selected, Some(2));
        app.previous(1);
        assert_eq!(app.list_selected, Some(1));
        app.previous(1);
        assert_eq!(app.list_selected, Some(0));
    }

    #[test]
    fn test_skip_deleted_folder() {
        let mut app = App::new(ScanConfig {
            dry_run: true,
            ..Default::default()
        });
        app.folders = vec![make_folder("/tmp/deleted", 0, FolderStatus::Deleted)];
        app.apply_sort();
        app.delete_selected();
        assert_eq!(app.stats.total_deleted, 0);
    }

    #[test]
    fn test_process_scan_results() {
        let mut app = App::new(ScanConfig::default());
        let folder = make_folder("/tmp/test_nm", 512, FolderStatus::Pending);
        app.process_scan_results(vec![folder]);
        assert_eq!(app.stats.total_found, 1);
        assert!(app.scan_complete);
    }

    #[test]
    fn test_real_index() {
        let mut app = setup_filtered_app();
        app.set_search_query("/a".to_string());
        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.real_index(0), Some(1));
    }

    #[test]
    fn test_selected_real_index() {
        let mut app = setup_filtered_app();
        app.list_selected = Some(0);
        assert!(app.selected_real_index().is_some());
    }

    #[test]
    fn test_sort_indicator() {
        let mut app = setup_filtered_app();
        let ind = app.sort_indicator();
        assert!(ind.contains("Size"));
        assert!(ind.contains('▾'));
        app.toggle_sort_direction();
        let ind = app.sort_indicator();
        assert!(ind.contains('▴'));
    }

    #[test]
    fn test_start_scan() {
        let mut app = App::new(ScanConfig::default());
        app.start_scan();
        assert!(!app.scan_complete);
        assert_eq!(app.status_message, "Scanning...");
    }
}
