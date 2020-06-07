use std::fmt::Write;
use std::time::Instant;

use sysinfo::{ProcessorExt, RefreshKind, System, SystemExt};
use tui::backend::Backend;
use tui::buffer::Buffer;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Paragraph, Text, Widget};
use tui::Frame;

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
            write!(res, "{} {}{}, ", n, unit, p).unwrap();
            secs %= div;
        }
    }
    let hours = secs / (60 * 60);
    secs %= 60 * 60;
    let minutes = secs / 60;
    secs %= 60;
    write!(res, "{:0>2}:{:0>2}:{:0>2}", hours, minutes, secs).unwrap();
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

pub struct SystemMonitor {
    system: System,
}

impl SystemMonitor {
    pub fn new() -> SystemMonitor {
        SystemMonitor {
            system: System::new_with_specifics(RefreshKind::new().with_cpu().with_memory()),
        }
    }

    pub fn on_tick(&mut self, _tick: Instant) {
        self.system.refresh_cpu();
        self.system.refresh_memory();
    }

    pub fn draw(&self, frame: &mut Frame<impl Backend>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(1), // load average
                    Constraint::Length(1), // uptime
                    Constraint::Length(1), // blank
                    Constraint::Length(2), // CPU meter
                    Constraint::Length(2), // memory meter
                ]
                .as_ref(),
            )
            .split(area);

        let value_style = Style::default()
            .fg(Color::LightCyan)
            .modifier(Modifier::BOLD);

        let avg = self.system.get_load_average();
        let load_average = format!("{} {} {}", avg.one, avg.five, avg.fifteen);
        frame.render_widget(
            Paragraph::new([Text::raw("LA "), Text::styled(load_average, value_style)].iter()),
            chunks[0],
        );

        let uptime = format_time(self.system.get_uptime());
        frame.render_widget(
            Paragraph::new([Text::raw("UP "), Text::styled(uptime, value_style)].iter()),
            chunks[1],
        );

        let cpu_usage = self
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

        let mem_usage =
            (100 * self.system.get_used_memory() / self.system.get_total_memory()) as u16;
        frame.render_widget(
            Meter {
                label: "Memory",
                percentage: mem_usage,
                style: value_style,
            },
            chunks[4],
        );
    }
}
