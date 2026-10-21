//! Host resource readout for the bottom of the sidebar.
//!
//! The sample is of the machine this client runs on, not the server's host. That
//! makes it TUI presentation state, so it stays out of the runtime protocol and no
//! codec changes: a client attached over SSH reports the machine in front of the
//! person reading it.
//!
//! Sampling happens on the client's timer tick and never on the render path, which
//! only reads percentages already computed. The render path is pane-scaled and may
//! not do I/O.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::*;
use crate::platform::{GpuUtilizationSource, HostResourceCounters};

/// How often the host is sampled. CPU is a delta between two samples, so the first
/// tick after start has memory and GPU but no CPU figure yet.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(1000);

/// Samples kept for the sparkline, wider than any sidebar so the strip can draw
/// whatever part of it fits.
const HISTORY_SAMPLES: usize = 64;

/// Rows the full readout occupies: a divider and one row per metric.
pub(crate) const STRIP_ROWS: u16 = 4;

/// Rows the collapsed readout occupies.
pub(crate) const COMPACT_ROWS: u16 = 1;

/// Width a metric row needs before a sparkline is worth drawing beside it.
const SPARKLINE_MIN_WIDTH: u16 = 14;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HostResourceReadout {
    pub cpu_percent: Option<u8>,
    pub memory_percent: Option<u8>,
    pub gpu_percent: Option<u8>,
}

/// Samples the host on a fixed interval and keeps the last readout plus a short
/// history of each metric.
#[derive(Debug, Default)]
pub(crate) struct HostResourceMonitor {
    previous: Option<HostResourceCounters>,
    sampled_at: Option<Instant>,
    readout: HostResourceReadout,
    cpu_history: VecDeque<u8>,
    memory_history: VecDeque<u8>,
    gpu_history: VecDeque<u8>,
}

impl HostResourceMonitor {
    /// Sample the host if the interval has elapsed. Returns true when the readout
    /// changed, so the caller can repaint only then.
    pub(crate) fn sample(&mut self, now: Instant, source: GpuUtilizationSource) -> bool {
        if self
            .sampled_at
            .is_some_and(|last| now.saturating_duration_since(last) < SAMPLE_INTERVAL)
        {
            return false;
        }
        self.sampled_at = Some(now);
        let Some(current) = crate::platform::host_resource_counters(source) else {
            return false;
        };
        let readout = Self::readout_from(self.previous, current);
        self.previous = Some(current);
        push_sample(&mut self.cpu_history, readout.cpu_percent);
        push_sample(&mut self.memory_history, readout.memory_percent);
        push_sample(&mut self.gpu_history, readout.gpu_percent);
        let changed = readout != self.readout;
        self.readout = readout;
        changed
    }

    /// Turn two counter samples into percentages. Pure, so the arithmetic is
    /// testable without a host to sample.
    fn readout_from(
        previous: Option<HostResourceCounters>,
        current: HostResourceCounters,
    ) -> HostResourceReadout {
        let cpu_percent = previous.and_then(|previous| {
            let total = current
                .cpu_total_ticks
                .checked_sub(previous.cpu_total_ticks)?;
            let busy = current
                .cpu_busy_ticks
                .saturating_sub(previous.cpu_busy_ticks);
            // A zero window means the clock has not ticked between samples, and a
            // busy count above the window means the counter was reset under us.
            (total > 0 && busy <= total).then(|| percent_of(busy, total))
        });
        let memory_percent = (current.memory_total_bytes > 0).then(|| {
            percent_of(
                current.memory_used_bytes.min(current.memory_total_bytes),
                current.memory_total_bytes,
            )
        });
        HostResourceReadout {
            cpu_percent,
            memory_percent,
            gpu_percent: current.gpu_percent.map(|percent| percent.min(100)),
        }
    }

    pub(crate) fn readout(&self) -> HostResourceReadout {
        self.readout
    }
}

impl ClientShellState {
    /// Sample the host on the client's timer tick, when the readout is on. Returns
    /// true when the numbers moved, so the caller repaints only then rather than
    /// once a second regardless.
    pub(crate) fn tick_host_resources(&mut self, now: Instant) -> bool {
        if !self.config.host_resources.enabled {
            return false;
        }
        let source = GpuUtilizationSource::from(self.config.host_resources.gpu_source);
        self.host_resources.sample(now, source)
    }
}

/// Record a sample, dropping the oldest once the window is full. A metric the
/// platform could not read records nothing, so its sparkline stays empty rather
/// than filling with zeroes it never measured.
fn push_sample(history: &mut VecDeque<u8>, percent: Option<u8>) {
    let Some(percent) = percent else {
        return;
    };
    if history.len() >= HISTORY_SAMPLES {
        history.pop_front();
    }
    history.push_back(percent);
}

fn percent_of(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    let scaled = part.saturating_mul(100) / whole;
    u8::try_from(scaled.min(100)).unwrap_or(100)
}

/// One block glyph standing for a percentage, for the collapsed sidebar where
/// there is no room for digits.
fn gauge(percent: Option<u8>) -> char {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    match percent {
        None => '·',
        Some(percent) => {
            let index = usize::from(percent.min(100)) * (BLOCKS.len() - 1) / 100;
            BLOCKS[index]
        }
    }
}

/// A sparkline of the most recent samples that fit, oldest on the left.
fn sparkline(history: &VecDeque<u8>, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    history
        .iter()
        .rev()
        .take(width)
        .rev()
        .map(|percent| gauge(Some(*percent)))
        .collect()
}

/// Colour a percentage by how much headroom is left, reusing the palette's own
/// status colours rather than introducing new ones.
fn percent_color(percent: Option<u8>, palette: &Palette) -> Color {
    match percent {
        None => palette.overlay0,
        Some(percent) if percent >= 85 => palette.red,
        Some(percent) if percent >= 60 => palette.yellow,
        Some(_) => palette.green,
    }
}

fn percent_text(percent: Option<u8>) -> String {
    percent.map_or_else(|| "  --".to_string(), |percent| format!("{percent:>3}%"))
}

/// Draw the readout at the bottom of an expanded sidebar.
pub(crate) fn render_host_resources(
    buffer: &mut Buffer,
    area: Rect,
    monitor: &HostResourceMonitor,
    palette: &Palette,
) {
    if area.is_empty() {
        return;
    }
    // The sidebar's own separator column stays clear.
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return;
    }
    for x in content.x..content.right() {
        if let Some(cell) = buffer.cell_mut((x, content.y)) {
            cell.set_symbol("─");
            cell.set_style(Style::default().fg(palette.surface_dim));
        }
    }
    let readout = monitor.readout();
    let rows = [
        ("CPU", readout.cpu_percent, &monitor.cpu_history),
        ("MEM", readout.memory_percent, &monitor.memory_history),
        ("GPU", readout.gpu_percent, &monitor.gpu_history),
    ];
    for (index, (label, percent, history)) in rows.iter().enumerate() {
        let Ok(offset) = u16::try_from(index + 1) else {
            break;
        };
        let y = content.y.saturating_add(offset);
        if y >= content.bottom() {
            break;
        }
        let row = Rect::new(content.x, y, content.width, 1);
        let mut spans = vec![
            Span::styled(format!(" {label}"), Style::default().fg(palette.subtext0)),
            Span::styled(
                percent_text(*percent),
                Style::default().fg(percent_color(*percent, palette)),
            ),
        ];
        if row.width >= SPARKLINE_MIN_WIDTH {
            let room = usize::from(row.width.saturating_sub(SPARKLINE_MIN_WIDTH - 5));
            let graph = sparkline(history, room);
            if !graph.is_empty() {
                spans.push(Span::styled(
                    format!(" {graph}"),
                    Style::default().fg(palette.overlay0),
                ));
            }
        }
        Paragraph::new(Line::from(spans)).render(row, buffer);
    }
}

/// Draw the readout in a collapsed sidebar, where three digits do not fit: one
/// gauge glyph per metric, CPU then memory then GPU.
pub(crate) fn render_host_resources_compact(
    buffer: &mut Buffer,
    area: Rect,
    monitor: &HostResourceMonitor,
    palette: &Palette,
) {
    if area.is_empty() {
        return;
    }
    let readout = monitor.readout();
    let metrics = [
        readout.cpu_percent,
        readout.memory_percent,
        readout.gpu_percent,
    ];
    let spans = metrics
        .iter()
        .map(|percent| {
            Span::styled(
                gauge(*percent).to_string(),
                Style::default().fg(percent_color(*percent, palette)),
            )
        })
        .collect::<Vec<_>>();
    let row = Rect::new(area.x, area.y, area.width.saturating_sub(1), 1);
    if row.is_empty() {
        return;
    }
    Paragraph::new(Line::from(spans)).render(row, buffer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(
        busy: u64,
        total: u64,
        used: u64,
        memory: u64,
        gpu: Option<u8>,
    ) -> HostResourceCounters {
        HostResourceCounters {
            cpu_busy_ticks: busy,
            cpu_total_ticks: total,
            memory_used_bytes: used,
            memory_total_bytes: memory,
            gpu_percent: gpu,
        }
    }

    #[test]
    fn cpu_needs_two_samples_and_survives_a_counter_reset() {
        let first = counters(100, 1_000, 2, 4, Some(30));
        // No previous sample: memory and GPU are available immediately, CPU is not.
        let readout = HostResourceMonitor::readout_from(None, first);
        assert_eq!(readout.cpu_percent, None);
        assert_eq!(readout.memory_percent, Some(50));
        assert_eq!(readout.gpu_percent, Some(30));

        // A quarter of the ticks in the window were busy.
        let second = counters(150, 1_200, 3, 4, Some(0));
        let readout = HostResourceMonitor::readout_from(Some(first), second);
        assert_eq!(readout.cpu_percent, Some(25));
        assert_eq!(readout.memory_percent, Some(75));
        assert_eq!(readout.gpu_percent, Some(0));

        // Counters that went backwards, or a window with no elapsed ticks, must not
        // produce a percentage rather than a nonsense one.
        let backwards = counters(10, 50, 1, 4, None);
        assert_eq!(
            HostResourceMonitor::readout_from(Some(second), backwards).cpu_percent,
            None
        );
        assert_eq!(
            HostResourceMonitor::readout_from(Some(second), second).cpu_percent,
            None,
            "an identical sample is a zero-length window"
        );
        let busier_than_the_window = counters(1_000, 1_300, 1, 4, None);
        assert_eq!(
            HostResourceMonitor::readout_from(Some(second), busier_than_the_window).cpu_percent,
            None
        );

        // Memory with no total reported is unknown, not zero.
        assert_eq!(
            HostResourceMonitor::readout_from(None, counters(0, 1, 5, 0, None)).memory_percent,
            None
        );
        // A used figure above the total is clamped instead of overflowing.
        assert_eq!(
            HostResourceMonitor::readout_from(None, counters(0, 1, 9, 4, None)).memory_percent,
            Some(100)
        );
    }

    #[test]
    fn gauges_and_sparklines_stay_inside_their_width() {
        assert_eq!(gauge(None), '·');
        assert_eq!(gauge(Some(0)), '▁');
        assert_eq!(gauge(Some(100)), '█');
        assert_eq!(
            gauge(Some(200)),
            '█',
            "a bad reading clamps rather than panics"
        );

        let history: VecDeque<u8> = (0..10u8).map(|step| step * 10).collect();
        assert_eq!(sparkline(&history, 0), "");
        assert_eq!(sparkline(&history, 3).chars().count(), 3);
        assert_eq!(
            sparkline(&history, 100).chars().count(),
            10,
            "a window wider than the history shows all of it"
        );
        // The newest sample is on the right.
        // 80% and 90% of eight blocks are the sixth and seventh, not the top two.
        assert_eq!(sparkline(&history, 2), "▆▇");
        assert_eq!(sparkline(&VecDeque::new(), 5), "");
    }

    #[test]
    fn percent_text_is_right_aligned_and_fixed_width() {
        assert_eq!(percent_text(None), "  --");
        assert_eq!(percent_text(Some(0)), "  0%");
        assert_eq!(percent_text(Some(7)), "  7%");
        assert_eq!(percent_text(Some(100)), "100%");
        for percent in 0..=100u8 {
            assert_eq!(percent_text(Some(percent)).chars().count(), 4);
        }
    }

    fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
        (0..width)
            .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
            .collect()
    }

    #[test]
    fn the_strip_draws_a_divider_then_one_row_per_metric() {
        let mut monitor = HostResourceMonitor::default();
        monitor.readout = HostResourceReadout {
            cpu_percent: Some(7),
            memory_percent: Some(64),
            gpu_percent: None,
        };
        monitor.cpu_history = (0..5u8).map(|step| step * 20).collect();
        monitor.memory_history = (0..5u8).map(|_| 50).collect();
        monitor.gpu_history = VecDeque::new();
        let area = Rect::new(0, 0, 24, STRIP_ROWS);
        let mut buffer = Buffer::empty(area);
        render_host_resources(&mut buffer, area, &monitor, &Palette::catppuccin());

        let rows: Vec<String> = (0..STRIP_ROWS)
            .map(|y| row_text(&buffer, y, area.width))
            .collect();
        assert!(
            rows[0].starts_with("─"),
            "a divider separates it: {:?}",
            rows[0]
        );
        assert!(
            rows[1].contains("CPU") && rows[1].contains("7%"),
            "{:?}",
            rows[1]
        );
        assert!(
            rows[2].contains("MEM") && rows[2].contains("64%"),
            "{:?}",
            rows[2]
        );
        assert!(
            rows[3].contains("GPU") && rows[3].contains("--"),
            "an unavailable metric reads as unknown, not zero: {:?}",
            rows[3]
        );
        assert!(
            rows[1].chars().any(|glyph| "▁▂▃▄▅▆▇█".contains(glyph)),
            "the CPU row graphs its own history: {:?}",
            rows[1]
        );
        assert!(
            rows[2].chars().any(|glyph| "▁▂▃▄▅▆▇█".contains(glyph)),
            "so does memory: {:?}",
            rows[2]
        );
        assert!(
            !rows[3].chars().any(|glyph| "▁▂▃▄▅▆▇█".contains(glyph)),
            "a metric with no history graphs nothing rather than a flat floor: {:?}",
            rows[3]
        );
        // The sidebar's separator column is left to the sidebar itself.
        for row in &rows {
            assert_eq!(row.chars().count(), usize::from(area.width));
        }
    }

    #[test]
    fn the_collapsed_strip_is_three_gauges_in_three_columns() {
        let mut monitor = HostResourceMonitor::default();
        monitor.readout = HostResourceReadout {
            cpu_percent: Some(0),
            memory_percent: Some(100),
            gpu_percent: None,
        };
        let area = Rect::new(0, 0, 4, COMPACT_ROWS);
        let mut buffer = Buffer::empty(area);
        render_host_resources_compact(&mut buffer, area, &monitor, &Palette::catppuccin());
        let row = row_text(&buffer, 0, area.width);
        assert_eq!(row, "▁█· ", "cpu, memory, gpu, then the separator column");
    }

    #[test]
    fn an_empty_area_draws_nothing() {
        let monitor = HostResourceMonitor::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        render_host_resources(
            &mut buffer,
            Rect::default(),
            &monitor,
            &Palette::catppuccin(),
        );
        render_host_resources_compact(
            &mut buffer,
            Rect::default(),
            &monitor,
            &Palette::catppuccin(),
        );
        assert_eq!(row_text(&buffer, 0, 4).trim(), "");
    }

    #[test]
    fn each_history_is_bounded_and_skips_unreadable_metrics() {
        let mut history = VecDeque::new();
        for sample in 0..(HISTORY_SAMPLES * 2) {
            push_sample(&mut history, Some(u8::try_from(sample % 101).unwrap_or(0)));
        }
        assert_eq!(history.len(), HISTORY_SAMPLES);
        // The oldest samples fall off the front, so the newest survives.
        assert_eq!(
            history.back().copied(),
            Some(u8::try_from((HISTORY_SAMPLES * 2 - 1) % 101).unwrap_or(0))
        );

        let mut absent = VecDeque::new();
        push_sample(&mut absent, None);
        assert!(absent.is_empty(), "an unreadable metric records nothing");
    }
}
