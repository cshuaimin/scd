use sysinfo::{ProcessorExt, System, SystemExt};

use tui::buffer::Buffer;
use tui::layout::Rect;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::widgets::StatefulWidget;

fn draw_progress_bar(label: &str, mut percentage: u16, area: Rect, buf: &mut Buffer) {
    if percentage > 100 {
        percentage = 0;
    }
    let width = area.width - 5;
    let end = area.left() + width;
    let sep = end * percentage / 100;

    buf.set_string(area.left(), area.top(), label, Style::default());
    for x in area.left()..sep {
        buf.get_mut(x, area.top() + 1)
            .set_fg(Color::Green)
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
        format!("{:>3}%", percentage),
        Style::default(),
    );
}

pub struct SystemMonitor;

impl StatefulWidget for SystemMonitor {
    type State = System;

    fn render(self, area: Rect, buf: &mut Buffer, system: &mut Self::State) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Min(0),
                ]
                .as_ref(),
            )
            .split(area);
        let cpu_usage = system.get_global_processor_info().get_cpu_usage().round() as u16;
        draw_progress_bar("CPU", cpu_usage, chunks[0], buf);
        let mem_usage = (100 * system.get_used_memory() / system.get_total_memory()) as u16;
        draw_progress_bar("Memory", mem_usage, chunks[1], buf);
    }
}
