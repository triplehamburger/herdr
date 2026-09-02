use super::*;

fn unified_config() -> ClientShellConfig {
    let mut config = Config::default();
    config.ui.sidebar_mode = crate::config::SidebarModeConfig::Unified;
    ClientShellConfig::from_config(&config)
}

/// Two spaces, three agents, so nesting and grouping are both exercised.
fn nested_snapshot() -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    snapshot.workspaces.push(ClientShellWorkspace {
        workspace_id: "ws_2".into(),
        active_tab_id: "tab_2".into(),
        new_workspace_cwd: "/other".into(),
        number: 2,
        label: "other-space".into(),
        custom_label: false,
        branch: Some("main".into()),
        git_ahead_behind: None,
        tokens: Vec::new(),
        worktree: None,
        focused: false,
        agent_status: AgentStatus::Done,
    });
    snapshot.tabs.push(ClientShellTab {
        tab_id: "tab_2".into(),
        workspace_id: "ws_2".into(),
        number: 1,
        label: "1".into(),
        custom_label: false,
        zoomed: false,
        focused: false,
        agent_status: AgentStatus::Done,
    });
    snapshot.agents = vec![
        agent(
            "pane_1",
            "ws_1",
            "tab_1",
            "alpha-agent",
            AgentStatus::Working,
        ),
        agent("pane_2", "ws_2", "tab_2", "beta-agent", AgentStatus::Done),
        agent("pane_3", "ws_2", "tab_2", "gamma-agent", AgentStatus::Done),
    ];
    snapshot
}

fn agent(
    pane_id: &str,
    workspace_id: &str,
    tab_id: &str,
    title: &str,
    status: AgentStatus,
) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
        tab_id: tab_id.into(),
        agent: Some("claude".into()),
        display_agent: None,
        name: None,
        title: None,
        terminal_title: Some(title.into()),
        terminal_title_stripped: Some(title.into()),
        agent_status: status,
        focused: false,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
    }
}

fn row_text(frame: &FrameData, row: u16) -> String {
    let width = usize::from(frame.width);
    let start = usize::from(row) * width;
    frame.cells[start..start + width]
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>()
}

fn sidebar_text(frame: &FrameData) -> String {
    (0..frame.height)
        .map(|row| row_text(frame, row))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn unified_mode_nests_agents_under_their_space_and_drops_the_agents_panel() {
    let mut state = ClientShellState::new(unified_config());
    state.set_snapshot(Box::new(nested_snapshot()));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("composed frame");
    let text = sidebar_text(&frame);

    // The separate agents panel and its header are gone.
    assert!(
        !text.contains(" agents"),
        "unified sidebar must not draw the agents panel header:\n{text}"
    );
    assert!(
        state.hits.agent_sort_toggle.is_empty(),
        "the agents panel sort toggle must not be hit-testable in unified mode"
    );
    assert!(
        state.hits.sidebar_section_divider.is_empty(),
        "unified mode has one section, so there is no divider to drag"
    );

    // Every agent is still reachable, now as a nested row.
    let hit_panes = state
        .hits
        .agents
        .iter()
        .map(|(_, pane_id)| pane_id.as_str())
        .collect::<Vec<_>>();
    assert!(hit_panes.contains(&"pane_1"));
    assert!(hit_panes.contains(&"pane_2"));
    assert!(hit_panes.contains(&"pane_3"));

    // Nested rows are drawn as children, below the space that owns them.
    assert!(
        text.contains("beta-agent") && text.contains("gamma-agent"),
        "nested agent rows should render their titles:\n{text}"
    );
}

#[test]
fn split_mode_is_unchanged() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(nested_snapshot()));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("composed frame");
    let text = sidebar_text(&frame);

    assert!(
        text.contains(" agents"),
        "split mode keeps the agents panel:\n{text}"
    );
    assert!(
        !state.hits.sidebar_section_divider.is_empty(),
        "split mode keeps the draggable section divider"
    );
    assert!(
        state
            .hits
            .workspaces
            .iter()
            .all(|hit| hit.space_toggle.is_none()),
        "split mode must not offer per-space expand toggles"
    );
}

#[test]
fn collapsing_a_space_hides_its_agents_but_keeps_their_attention_counts() {
    let mut state = ClientShellState::new(unified_config());
    state.set_snapshot(Box::new(nested_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("composed frame");

    let toggle = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .and_then(|hit| hit.space_toggle.clone())
        .expect("a space with agents offers an expand/collapse toggle");

    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: toggle.0.x,
        row: toggle.0.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(outcome.repaint, "collapsing a space repaints the sidebar");
    assert!(state.collapsed_spaces.contains("ws_2"));

    let frame = state.compose(106, 30).expect("recomposed frame");
    let text = sidebar_text(&frame);
    assert!(
        !text.contains("beta-agent") && !text.contains("gamma-agent"),
        "a collapsed space hides its agent rows:\n{text}"
    );
    assert!(
        text.contains("alpha-agent"),
        "collapsing one space must not affect another:\n{text}"
    );
    // Two done agents are hidden, so the row carries the count in their place.
    assert!(
        text.contains('2'),
        "a collapsed space reports how many agents want attention:\n{text}"
    );

    // Clicking again expands it back.
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: toggle.0.x,
        row: toggle.0.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(!state.collapsed_spaces.contains("ws_2"));
}

#[test]
fn a_space_with_no_agents_offers_no_toggle() {
    let mut state = ClientShellState::new(unified_config());
    let mut snapshot = nested_snapshot();
    snapshot.agents.retain(|agent| agent.workspace_id == "ws_1");
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("composed frame");

    let toggle = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .and_then(|hit| hit.space_toggle.clone());
    assert!(
        toggle.is_none(),
        "an empty space has nothing to expand, so it draws no toggle"
    );
}

#[test]
fn the_agent_label_shows_only_on_the_focused_session() {
    let mut state = ClientShellState::new(unified_config());
    let mut snap = nested_snapshot();
    // pane_2 is the session the user is on.
    for agent in snap.agents.iter_mut() {
        agent.focused = agent.pane_id == "pane_2";
    }
    state.set_snapshot(Box::new(snap));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("composed frame");
    let text = sidebar_text(&frame);

    // Every agent is still listed by title...
    assert!(text.contains("alpha-agent"));
    assert!(text.contains("beta-agent"));
    assert!(text.contains("gamma-agent"));

    // ...but "claude" appears exactly once, on the focused row.
    assert_eq!(
        text.matches("claude").count(),
        1,
        "the agent label belongs to the focused session only:\n{text}"
    );
}

#[test]
fn split_mode_still_labels_every_agent() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut snap = nested_snapshot();
    for agent in snap.agents.iter_mut() {
        agent.focused = agent.pane_id == "pane_2";
    }
    state.set_snapshot(Box::new(snap));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("composed frame");
    let text = sidebar_text(&frame);
    assert_eq!(
        text.matches("claude").count(),
        3,
        "split mode must keep labelling every agent:\n{text}"
    );
}

#[test]
fn nested_titles_render_brighter_than_the_rows_around_them() {
    const BOLD: u16 = 1 << 0;
    const DIM: u16 = 1 << 1;

    let mut state = ClientShellState::new(unified_config());
    state.set_snapshot(Box::new(nested_snapshot()));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 30).expect("composed frame");

    // Find the cell where a nested agent title starts and check it is not dimmed.
    let width = usize::from(frame.width);
    let mut found = false;
    for row in 0..frame.height {
        let text = row_text(&frame, row);
        if let Some(column) = text.find("beta-agent") {
            let cell = &frame.cells[usize::from(row) * width + column];
            assert_eq!(
                cell.modifier & DIM,
                0,
                "a nested agent title must not be dimmed"
            );
            assert_ne!(
                cell.modifier & BOLD,
                0,
                "a nested agent title should be emphasised"
            );
            found = true;
        }
    }
    assert!(found, "nested agent title was not rendered");
}
