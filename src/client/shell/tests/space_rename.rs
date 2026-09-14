use super::*;

fn rename_overlay(state: &ClientShellState) -> Option<&ClientRenameOverlay> {
    match state.overlay.as_ref() {
        Some(ClientShellOverlay::Rename(rename)) => Some(rename),
        _ => None,
    }
}

fn workspace_rename_action(outcome: &ClientShellInput) -> Option<(&str, &str)> {
    outcome.actions.iter().find_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => match &request.method {
            crate::api::schema::Method::WorkspaceRename(params) => {
                Some((params.workspace_id.as_str(), params.label.as_str()))
            }
            _ => None,
        },
        _ => None,
    })
}

fn open_focused_rename(state: &mut ClientShellState) {
    state.set_snapshot(Box::new(snapshot()));
    let prefix = state.handle_input_bytes(b"\x1b[98;5u");
    assert!(prefix.requests.is_empty() && prefix.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let rename = state.handle_input_bytes(b"\x1b[114;5u");
    assert!(rename.requests.is_empty() && rename.actions.is_empty());
    assert!(rename.repaint);
}

#[test]
fn prefix_ctrl_r_kitty_opens_rename_overlay_for_focused_space() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    open_focused_rename(&mut state);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    let Some(rename) = rename_overlay(&state) else {
        panic!("prefix+ctrl+r should open the rename workspace overlay");
    };
    assert_eq!(rename.title, "rename workspace");
    assert_eq!(rename.input, "client-shell");
    assert!(matches!(
        rename.target,
        ClientRenameTarget::Workspace {
            ref workspace_id
        } if workspace_id == "ws_1"
    ));
    // A real kitty release for the captured chord must not reach the pane.
    let release = state.handle_input_bytes(b"\x1b[114;5:3u");
    assert!(release.requests.is_empty() && release.actions.is_empty());
}

#[test]
fn prefix_ctrl_r_legacy_bytes_open_rename_overlay() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let prefix = state.handle_input_bytes(b"\x02");
    assert!(prefix.requests.is_empty() && prefix.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let rename = state.handle_input_bytes(b"\x12");
    assert!(rename.requests.is_empty() && rename.actions.is_empty());
    assert!(rename.repaint);
    assert!(matches!(
        rename_overlay(&state),
        Some(ClientRenameOverlay {
            target: ClientRenameTarget::Workspace { ref workspace_id },
            ..
        }) if workspace_id == "ws_1"
    ));
    assert_eq!(state.mode, ClientShellMode::Terminal);
}

#[test]
fn raw_ctrl_r_still_reaches_the_focused_pane() {
    for bytes in [b"\x12".as_slice(), b"\x1b[114;5u".as_slice()] {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.set_snapshot(Box::new(snapshot()));
        let outcome = state.handle_input_bytes(bytes);
        assert!(
            matches!(
                outcome.requests.as_slice(),
                [ClientMessage::ClientShellPaneInput { pane_id, .. }]
                    if pane_id == "pane_1"
            ),
            "raw ctrl+r {bytes:?} must keep passing through to the focused pane"
        );
        assert!(outcome.actions.is_empty());
        assert!(rename_overlay(&state).is_none());
    }
}

#[test]
fn prefix_ctrl_r_types_and_commits_the_rename() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    open_focused_rename(&mut state);
    for bytes in [b"\x15".as_slice(), b"renamed".as_slice()] {
        let typed = state.handle_input_bytes(bytes);
        assert!(typed.requests.is_empty() && typed.actions.is_empty());
    }
    let Some(rename) = rename_overlay(&state) else {
        panic!("rename overlay should stay open while typing");
    };
    assert_eq!(rename.input, "renamed");
    let save = state.handle_input_bytes(b"\r");
    assert!(save.requests.is_empty());
    assert!(rename_overlay(&state).is_none());
    assert_eq!(
        workspace_rename_action(&save),
        Some(("ws_1", "renamed")),
        "committed rename should use the workspace.rename endpoint method"
    );
}

#[test]
fn prefix_ctrl_r_escape_cancels_without_rename() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    open_focused_rename(&mut state);
    let escape = state.handle_input_bytes(b"\x1b");
    assert!(escape.requests.is_empty() && escape.actions.is_empty());
    assert!(escape.repaint);
    assert!(rename_overlay(&state).is_none());
    assert_eq!(workspace_rename_action(&escape), None);
    // The escape was consumed by the overlay, so the pane saw nothing.
    assert!(escape.requests.is_empty());
    // With the overlay gone, raw ctrl+r immediately reaches the pane again.
    let stray = state.handle_input_bytes(b"\x12");
    assert!(matches!(
        stray.requests.as_slice(),
        [ClientMessage::ClientShellPaneInput { pane_id, .. }] if pane_id == "pane_1"
    ));
}

#[test]
fn prefix_ctrl_r_empty_or_whitespace_commits_nothing() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    open_focused_rename(&mut state);
    // Clear the prefilled label, then commit empty.
    let _ = state.handle_input_bytes(b"\x15");
    let save = state.handle_input_bytes(b"\r");
    assert!(save.requests.is_empty());
    assert_eq!(workspace_rename_action(&save), None);
    assert!(rename_overlay(&state).is_none());

    // Whitespace-only labels are also ignored.
    open_focused_rename(&mut state);
    let _ = state.handle_input_bytes(b"\x15");
    let _ = state.handle_input_bytes(b"   ");
    let save = state.handle_input_bytes(b"\r");
    assert!(save.requests.is_empty());
    assert_eq!(workspace_rename_action(&save), None);
    assert!(rename_overlay(&state).is_none());
}

#[test]
fn prefix_ctrl_r_sends_long_labels_without_truncation() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    open_focused_rename(&mut state);
    let _ = state.handle_input_bytes(b"\x15");
    let long_label = "renamed_".repeat(40);
    let typed = state.handle_input_bytes(long_label.as_bytes());
    assert!(typed.requests.is_empty() && typed.actions.is_empty());
    let save = state.handle_input_bytes(b"\r");
    assert_eq!(
        workspace_rename_action(&save),
        Some(("ws_1", long_label.as_str()))
    );
}

#[test]
fn navigate_mode_ctrl_r_renames_the_selected_space() {
    let mut projected = snapshot();
    let mut second = projected.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    second.number = 2;
    second.label = "second".into();
    second.focused = false;
    projected.workspaces.push(second);

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.mode = ClientShellMode::Navigate;
    state.navigate_workspace_id = state.navigation_target(&ClientEndpointId::Local, "ws_2");

    // Navigate mode resolves prefix bindings against bare keys.
    let rename = state.handle_input_bytes(b"\x12");
    assert!(rename.requests.is_empty() && rename.actions.is_empty());
    assert!(rename.repaint);
    assert_eq!(state.mode, ClientShellMode::Terminal);
    assert!(state.navigate_workspace_id.is_none());
    let Some(overlay) = rename_overlay(&state) else {
        panic!("navigate-mode ctrl+r should rename the selected space");
    };
    assert_eq!(overlay.input, "second");
    assert!(matches!(
        overlay.target,
        ClientRenameTarget::Workspace {
            ref workspace_id
        } if workspace_id == "ws_2"
    ));
}

#[test]
fn navigate_mode_ctrl_r_on_foreign_endpoint_shows_notice() {
    use crate::client::endpoint::{ClientEndpointStatus, ProfileId, SavedSshEndpoint};

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let profile = SavedSshEndpoint {
        id: ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
        label: "Build".into(),
        target: "dev@build.example".into(),
        session: "agents".into(),
        enabled: true,
    };
    let remote = ClientEndpointId::Ssh(profile.id.clone());
    state.set_endpoint_catalog(&[profile]);
    state.set_endpoint_status(&remote, ClientEndpointStatus::Online);
    state.set_snapshot(Box::new(snapshot()));
    let mut remote_snapshot = snapshot();
    remote_snapshot.boot_id = "remote-boot".into();
    remote_snapshot.workspaces[0].label = "remote-workspace".into();
    state.set_endpoint_snapshot(&remote, Box::new(remote_snapshot));

    state.mode = ClientShellMode::Navigate;
    state.navigate_workspace_id = state.navigation_target(&remote, "ws_1");
    assert!(state.workspace_preview_action_blocked());

    let rename = state.handle_input_bytes(b"\x12");
    assert!(rename.requests.is_empty() && rename.actions.is_empty());
    assert!(rename_overlay(&state).is_none());
    // Navigate mode keeps the foreign selection and shows the shared
    // confirm-first notice instead of opening the rename overlay.
    assert_eq!(state.mode, ClientShellMode::Navigate);
    assert_eq!(state.workspace_action_id().as_deref(), Some("ws_1"));
    assert!(state
        .visible_endpoint_notice
        .as_ref()
        .is_some_and(|notice| notice.body.contains("press Enter")));
}

#[test]
fn prefix_ctrl_r_without_snapshot_is_a_no_op() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let prefix = state.handle_input_bytes(b"\x02");
    assert!(prefix.requests.is_empty() && prefix.actions.is_empty());
    assert_eq!(state.mode, ClientShellMode::Prefix);
    let rename = state.handle_input_bytes(b"\x12");
    assert!(rename.requests.is_empty() && rename.actions.is_empty());
    assert!(rename_overlay(&state).is_none());
}
