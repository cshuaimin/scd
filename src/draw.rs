use std::os::unix::fs::PermissionsExt;

use sysinfo::{ProcessorExt, SystemExt};
use tui::backend::Backend;
use tui::buffer::Buffer;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{List, Paragraph, Text, Widget};
use tui::Frame;

use crate::app::{App, Mode};

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
    if size == 0 {
        return "0B".to_string();
    }

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
    unreachable!()
}

fn strmode(mode: u32) -> String {
    #[link(name = "bsd")]
    extern "C" {
        fn strmode(mode: u32, bp: *mut u8);
    }
    let mut res = "N".repeat(12);
    unsafe {
        strmode(mode, res.as_mut_ptr());
    }
    res.pop();
    res
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(9),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(frame.size());
    draw_system_monitor(frame, app, chunks[0]);
    draw_file_manager(frame, app, chunks[1]);
    draw_bottom_line(frame, app, chunks[2]);
}

pub fn draw_system_monitor<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
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

pub fn draw_file_manager<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
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
            let is_selected = match app.files_marked.contains(&file.path) {
                true => "+",
                false => " ",
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
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

pub fn draw_bottom_line<B>(frame: &mut Frame<B>, app: &mut App, area: Rect)
where
    B: Backend,
{
    match &app.mode {
        Mode::Normal => {
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

            let texts = [Text::raw(format!(
                "{} {}/{}",
                match app.files_marked.len() {
                    0 => "".to_string(),
                    len => format!("M:{}", len),
                },
                app.list_state.selected().map(|i| i + 1).unwrap_or(0),
                app.files.len()
            ))];
            frame.render_widget(
                Paragraph::new(texts.iter()).alignment(Alignment::Right),
                area,
            );
        }
        Mode::Message { text, .. } => {
            let texts = [Text::styled(text, Style::default().fg(Color::LightYellow))];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
        Mode::Ask { prompt, .. } => {
            let texts = [Text::styled(prompt, Style::default().fg(Color::LightYellow))];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
        Mode::Input { prompt, input, .. } => {
            let texts = [
                Text::raw(prompt),
                Text::styled(input, Style::default().fg(Color::LightCyan)),
            ];
            frame.render_widget(Paragraph::new(texts.iter()), area);
        }
    }
}
