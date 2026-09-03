use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::{display_width, put_segment, put_text};
use super::{status_color, status_icon, ClientShellConfig, ShellHitMap};
use crate::protocol::{ClientShellAgent, ClientShellSnapshot};

/// One row: a status symbol and the name.
pub(super) const FORK_BAR_HEIGHT: u16 = 1;
/// Chips size to their name so more forks fit; a long name is clipped.
const FORK_CHIP_MAX_WIDTH: u16 = 20;
const FORK_CHIP_GAP: u16 = 1;

pub(super) struct ForkChip {
    pane_id: String,
    name: String,
    status: crate::api::schema::AgentStatus,
    focused: bool,
}

/// A fork is an agent another agent spawned through `agent.start`, which always
/// carries the name the spawner gave it. Agents a human launched by typing in a
/// pane have no name and stay out of the strip.
pub(super) fn fork_name(agent: &ClientShellAgent) -> Option<&str> {
    agent
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// Chips in snapshot order, so they do not jump around as agents change state.
pub(super) fn fork_chips(agents: &[ClientShellAgent]) -> Vec<ForkChip> {
    agents
        .iter()
        .filter_map(|agent| {
            Some(ForkChip {
                pane_id: agent.pane_id.clone(),
                name: fork_name(agent)?.to_owned(),
                status: agent.agent_status,
                focused: agent.focused,
            })
        })
        .collect()
}

pub(super) fn render_fork_bar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let palette = &config.palette;
    buffer.set_style(area, Style::default().bg(palette.panel_bg));
    let chips = fork_chips(&snapshot.agents);
    let mut x = area.x.saturating_add(1);
    for (index, chip) in chips.iter().enumerate() {
        let remaining = chips.len().saturating_sub(index);
        let more = format!("+{remaining}");
        let reserved = if remaining > 1 {
            display_width(&more).saturating_add(FORK_CHIP_GAP)
        } else {
            0
        };
        let chip_width = display_width(&chip.name)
            .saturating_add(4)
            .min(FORK_CHIP_MAX_WIDTH);
        if x.saturating_add(chip_width).saturating_add(reserved) > area.right() {
            put_text(
                buffer,
                x,
                area.y,
                area.right().saturating_sub(x),
                &more,
                Style::default()
                    .fg(palette.overlay0)
                    .bg(palette.panel_bg)
                    .add_modifier(Modifier::DIM),
            );
            break;
        }
        let rect = Rect::new(x, area.y, chip_width, area.height);
        render_fork_chip(buffer, rect, chip, config);
        hits.forks.push((rect, chip.pane_id.clone()));
        x = x.saturating_add(chip_width).saturating_add(FORK_CHIP_GAP);
    }
}

fn render_fork_chip(buffer: &mut Buffer, rect: Rect, chip: &ForkChip, config: &ClientShellConfig) {
    let palette = &config.palette;
    let bg = if chip.focused {
        palette.active_row_bg
    } else {
        palette.surface0
    };
    buffer.set_style(rect, Style::default().bg(bg));
    let x = put_segment(
        buffer,
        rect.x.saturating_add(1),
        rect.y,
        rect.right(),
        status_icon(chip.status, config.status_indicators),
        Style::default()
            .fg(status_color(chip.status, palette))
            .bg(bg)
            .add_modifier(if chip.focused {
                Modifier::empty()
            } else {
                Modifier::DIM
            }),
    );
    let x = x.saturating_add(1);
    put_text(
        buffer,
        x,
        rect.y,
        rect.right().saturating_sub(x),
        &chip.name,
        Style::default()
            .fg(if chip.focused {
                palette.text
            } else {
                palette.subtext0
            })
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::AgentStatus;

    fn agent(pane_id: &str, name: Option<&str>) -> ClientShellAgent {
        ClientShellAgent {
            pane_id: pane_id.into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: name.map(str::to_owned),
            display_agent: None,
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: AgentStatus::Working,
            state_change_seq: 1,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: false,
        }
    }

    #[test]
    fn only_spawned_agents_become_chips() {
        let chips = fork_chips(&[
            agent("pane_1", Some("analysis")),
            agent("pane_2", None),
            agent("pane_3", Some("   ")),
            agent("pane_4", Some("review")),
        ]);
        assert_eq!(
            chips
                .iter()
                .map(|chip| (chip.pane_id.as_str(), chip.name.as_str()))
                .collect::<Vec<_>>(),
            vec![("pane_1", "analysis"), ("pane_4", "review")]
        );
    }
}
