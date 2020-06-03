use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;

use strmode::strmode;
use sysinfo::{ProcessorExt, SystemExt};
use termion::color;
use termion::cursor;
use tui::backend::Backend;
use tui::buffer::Buffer;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{List, Paragraph, Text, Widget};
use tui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode};
use crate::task;

fn format_time(mut secs: u64) -> String {
    const UNITS: &[(u64, &str)] = &[
        (60 * 60 * 24 * 31, "month"),
        (60 * 60 * 24 * 7, "week"),
        (60 * 60 * 24, "day"),
    ];

    let mut res = String::new();
    for &(div, unit) in UNITS {
        if secs >= div {
            let n = secs / div;
            let p = if n > 1 { "s" } else { "" };
            res.push_str(&format!("{} {}{}, ", n, unit, p));
            secs %= div;
        }
    }
    let hours = secs / (60 * 60);
    secs %= 60 * 60;
    let minutes = secs / 60;
    secs %= 60;
    res.push_str(&format!("{:0>2}:{:0>2}:{:0>2}", hours, minutes, secs));
    res
}

fn format_size(size: u64) -> String {
    const UNITS: &[(u64, &str)] = &[
        (1024 * 1024 * 1024 * 1024, "T"),
        (1024 * 1024 * 1024, "G"),
        (1024 * 1024, "M"),
        (1024, "K"),
        (1, "B"),
    ];

    for &(div, unit) in UNITS {
        if size >= div {
            return format!("{:.1}{}", size as f32 / div as f32, unit);
        }
    }
    "0B".to_string()
}

struct Meter<'a> {
    label: &'a str,
    percentage: u16,
    style: Style,
}

impl<'a> Widget for Meter<'a> {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        if self.percentage > 100 {
            self.percentage = 0;
        }
        if area.width <= 5 {
            return;
        }

        let width = area.width - 5; // space + 100%
        let sep = area.left() + width * self.percentage / 100;
        let end = area.left() + width;

        buf.set_string(area.left(), area.top(), self.label, Style::default());
        for x in area.left()..sep {
            buf.get_mut(x, area.top() + 1)
                .set_fg(self.style.fg)
                .set_symbol("■");
        }
        for x in sep..end {
            buf.get_mut(x, area.top() + 1)
                .set_fg(Color::DarkGray)
                .set_symbol("■");
        }
        buf.set_string(
            end + 1,
            area.top() + 1,
            format!("{:>3}%", self.percentage),
            self.style,
        );
    }
}

pub fn draw_ui<B>(frame: &mut Frame<B>, app: &mut App)
where
    B: Backend,
{
    // + 3: seperator, title, seperator
    let task_height = (app.tasks.len() as u16 + 3).min(frame.size().height / 3);
    let mut chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(8),
                Constraint::Min(0),
                Constraint::Length(task_height),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(frame.size());
    draw_system_monitor(frame, app, chunks[0]);
    draw_file_manager(frame, app, chunks[1]);
    if !app.tasks.is_empty() {
        chunks[2].y += 1;
        chunks[2].height -= 1;
        draw_tasks(frame, app, chunks[2]);
    }
    draw_bottom_line(frame, app, chunks[3]);
}

fn draw_system_monitor<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
where
    B: Backend,
{
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1), // load average
                Constraint::Length(1), // uptime
                Constraint::Length(1), // blank
                Constraint::Length(2), // CPU meter
                Constraint::Length(2), // memory meter
                Constraint::Min(0),
            ]
            .as_ref(),
        )
        .split(area);

    let value_style = Style::default()
        .fg(Color::LightCyan)
        .modifier(Modifier::BOLD);

    let avg = app.system.get_load_average();
    let load_average = format!("{} {} {}", avg.one, avg.five, avg.fifteen);
    frame.render_widget(
        Paragraph::new([Text::raw("LA "), Text::styled(load_average, value_style)].iter()),
        chunks[0],
    );

    let uptime = format_time(app.system.get_uptime());
    frame.render_widget(
        Paragraph::new([Text::raw("UP "), Text::styled(uptime, value_style)].iter()),
        chunks[1],
    );

    let cpu_usage = app
        .system
        .get_global_processor_info()
        .get_cpu_usage()
        .round() as u16;
    frame.render_widget(
        Meter {
            label: "CPU",
            percentage: cpu_usage,
            style: value_style,
        },
        chunks[3],
    );

    let mem_usage = (100 * app.system.get_used_memory() / app.system.get_total_memory()) as u16;
    frame.render_widget(
        Meter {
            label: "Memory",
            percentage: mem_usage,
            style: value_style,
        },
        chunks[4],
    );
}

fn draw_file_manager<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
where
    B: Backend,
{
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
        .split(area);

    frame.render_widget(
        Paragraph::new(
            [Text::styled(
                app.dir.to_str().unwrap(),
                Style::default().modifier(Modifier::UNDERLINED),
            )]
            .iter(),
        ),
        chunks[0],
    );

    let items = app
        .files
        .iter()
        .map(|file| {
            let color = if file.metadata.is_dir() {
                Color::Blue
            } else if file.metadata.permissions().mode() & 0o1 != 0 {
                Color::Green
            } else {
                Color::White
            };
            let is_selected = if app.files_marked.contains(&file.path) {
                "+"
            } else {
                " "
            };
            let icon = app.icons.get(file);
            let suffix = if file.metadata.is_dir() { "/" } else { "" };
            Text::styled(
                format!("{}{} {}{}", is_selected, icon, file.name, suffix),
                Style::default().fg(color),
            )
        })
        .collect::<Vec<_>>()
        .into_iter();
    let list = List::new(items).highlight_style(Style::default().fg(Color::Black).bg(Color::Blue));
    frame.render_stateful_widget(list, chunks[1], &mut app.file_list_state);
}

pub struct TaskListState {
    offset: usize,
    selected: Option<usize>,
}

impl Default for TaskListState {
    fn default() -> Self {
        Self {
            offset: 0,
            selected: None,
        }
    }
}

fn draw_tasks<B>(_frame: &mut Frame<B>, app: &mut App, area: Rect)
where
    B: Backend,
{
    let height = area.height as usize;
    let state = &mut app.task_list_state;
    // Make sure the list show the selected item
    state.offset = if let Some(selected) = state.selected {
        if selected >= height + state.offset - 1 {
            selected + 1 - height
        } else if selected < state.offset {
            selected
        } else {
            state.offset
        }
    } else {
        0
    };

    let max_status_width = app
        .tasks
        .iter()
        .map(|t| match &t.status {
            task::Status::Running(s) => s.width(),
            task::Status::Stopped | task::Status::Exited(_) => 1,
        })
        .max()
        .unwrap()
        .max("Status".len()) as u16;

    let mut stdout = io::stdout();

    macro_rules! draw {
        ($i:expr, $left:expr, $right_color:expr, $right:expr) => {{
            // `Goto` is (1,1)-based
            let y = area.top() + $i as u16 + 1;
            let left_pos = cursor::Goto(area.left() + 1, y);
            let right_pos = cursor::Goto(area.width - max_status_width, y);
            write!(stdout, "{}{}", left_pos, " ".repeat(area.width as usize)).unwrap();
            write!(stdout, "{}{}", left_pos, $left).unwrap();
            write!(
                stdout,
                "{} {}{:>w$}",
                right_pos,
                $right_color,
                $right,
                w = max_status_width as usize
            )
            .unwrap();
        }};
    }

    draw!(0, "Task", color::White.fg_str(), "Status");

    app.tasks
        .iter()
        .rev()
        .map(|task| {
            let (status_color, status) = match &task.status {
                task::Status::Running(s) => (color::White.fg_str(), s.as_str()),
                task::Status::Stopped => (color::LightYellow.fg_str(), "\u{f04c}"),
                task::Status::Exited(s) => {
                    if s.success() {
                        (color::LightCyan.fg_str(), "✓")
                    } else {
                        (color::LightRed.fg_str(), "✗")
                    }
                }
            };
            (task.rendered.as_str(), status_color, status)
        })
        .skip(state.offset)
        .take(height - 1)
        .enumerate()
        .for_each(|(i, (command, status_color, status))| {
            draw!(i + 1, command, status_color, status)
        });

    write!(
        stdout,
        "{}{}",
        cursor::Goto(area.left() + 1, area.bottom()),
        " ".repeat(area.width as usize)
    )
    .unwrap();
    stdout.flush().unwrap();
}

fn draw_bottom_line<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
where
    B: Backend,
{
    let prompt_style = Style::default().fg(Color::LightYellow);
    match &app.mode {
        Mode::Normal | Mode::Task => {
            if let Some(file) = app.selected() {
                let mode = strmode(file.metadata.permissions().mode());
                let size = format_size(file.metadata.len());
                let texts = [
                    Text::styled(mode, Style::default().fg(Color::LightGreen)),
                    Text::raw(" "),
                    Text::raw(size),
                ];
                frame.render_widget(
                    Paragraph::new(texts.iter()).alignment(Alignment::Left),
                    area,
                );
            }

            let mut text = String::new();
            if !app.filter.is_empty() {
                text.push_str("F:");
                text.push_str(&app.filter);
            }
            if !app.files_marked.is_empty() {
                text.push_str(" M:");
                text.push_str(&app.files_marked.len().to_string());
            }
            text.push_str(&format!(
                " {}/{}",
                app.file_list_state.selected().map(|i| i + 1).unwrap_or(0),
                app.files.len()
            ));
            frame.render_widget(
                Paragraph::new([Text::raw(text)].iter()).alignment(Alignment::Right),
                area,
            );
        }
        Mode::Message { text, .. } => {
            let texts = [Text::styled(text, prompt_style)];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
        Mode::Ask { prompt, .. } => {
            let texts = [Text::styled(prompt, prompt_style)];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
        Mode::Input { prompt, input, .. } => {
            let texts = [
                Text::styled(prompt, prompt_style),
                Text::styled(input, Style::default().fg(Color::LightCyan)),
            ];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
    }
}
