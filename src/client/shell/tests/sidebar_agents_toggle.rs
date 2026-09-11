use super::*;

fn agent(name: &str, status: AgentStatus, state_change_seq: u64) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some(name.into()),
        display_agent: Some(name.into()),
        agent: Some(name.into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: status,
        state_change_seq,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: false,
    }
}

fn disabled_config() -> Config {
    toml::from_str("[ui.sidebar.agents]\nenabled = false\n").expect("disabled agents config")
}

fn frame_text(frame: &FrameData) -> String {
    frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect()
}

#[test]
fn disabled_agents_panel_gives_spaces_the_full_expanded_sidebar() {
    let mut projected = snapshot();
    projected.agents.push(agent("pi", AgentStatus::Blocked, 1));

    let mut enabled = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    enabled.set_snapshot(Box::new(projected.clone()));
    enabled.set_pane_surface(surface());
    let frame = enabled.compose(106, 30).expect("expanded sidebar");
    let text = frame_text(&frame);
    assert!(text.contains(" spaces"));
    assert!(text.contains(" agents"));
    assert!(text.contains("grouped"));
    assert!(!enabled.hits.sidebar_section_divider.is_empty());
    assert!(!enabled.hits.agent_body.is_empty());
    assert!(!enabled.hits.agent_sort_toggle.is_empty());
    assert!(!enabled.hits.agents.is_empty());

    let mut disabled = ClientShellState::new(ClientShellConfig::from_config(&disabled_config()));
    disabled.set_snapshot(Box::new(projected));
    disabled.set_pane_surface(surface());
    let frame = disabled
        .compose(106, 30)
        .expect("expanded sidebar without agents");
    let text = frame_text(&frame);
    assert!(text.contains(" spaces"));
    assert!(!text.contains(" agents"));
    assert!(!text.contains("grouped"));
    assert!(disabled.hits.sidebar_section_divider.is_empty());
    assert!(disabled.hits.agent_body.is_empty());
    assert!(disabled.hits.agent_sort_toggle.is_empty());
    assert!(disabled.hits.agents.is_empty());
    assert!(!disabled.hits.workspaces.is_empty());
    assert_eq!(
        disabled.hits.workspace_body.bottom() + 1,
        disabled.hits.sidebar_divider.bottom(),
        "spaces section must fill the sidebar height when the agents panel is hidden"
    );
}

#[test]
fn disabled_agents_panel_hides_the_collapsed_sidebar_rail() {
    let mut projected = snapshot();
    let template = projected.workspaces[0].clone();
    projected.workspaces = (1..=12)
        .map(|number| ClientShellWorkspace {
            workspace_id: format!("ws_{number}"),
            number,
            label: format!("space-{number}"),
            focused: number == 1,
            ..template.clone()
        })
        .collect();
    projected.agents.push(agent("pi", AgentStatus::Blocked, 1));

    let mut enabled = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    enabled.sidebar_collapsed = true;
    enabled.set_snapshot(Box::new(projected.clone()));
    enabled.set_pane_surface(surface());
    enabled.compose(106, 20).expect("collapsed sidebar rail");
    assert_eq!(enabled.hits.workspaces.len(), 10);
    assert_eq!(enabled.hits.agents.len(), 1);

    let mut disabled = ClientShellState::new(ClientShellConfig::from_config(&disabled_config()));
    disabled.sidebar_collapsed = true;
    disabled.set_snapshot(Box::new(projected));
    disabled.set_pane_surface(surface());
    disabled
        .compose(106, 20)
        .expect("collapsed sidebar rail without agents");
    assert_eq!(disabled.hits.workspaces.len(), 12);
    assert!(disabled.hits.agents.is_empty());
}
