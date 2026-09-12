use super::*;

fn command_state(count: usize) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.command_spaces.enabled = true;
    state.config.agents.enabled = false;
    let mut snapshot = snapshot();
    snapshot.workspaces = (1..=count)
        .map(|number| {
            let mut space = snapshot.workspaces[0].clone();
            space.workspace_id = format!("ws_{number}");
            space.label = format!("Space {number}");
            space.number = number;
            space.focused = number == 1;
            space
        })
        .collect();
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state
}

fn assert_focus(outcome: &ClientShellInput, workspace: &str) {
    assert!(outcome.requests.is_empty());
    assert!(
        matches!(outcome.actions.as_slice(), [ClientShellAction::Endpoint { endpoint_id: ClientEndpointId::Local, request, .. }]
        if matches!(&request.method, crate::api::schema::Method::WorkspaceFocus(target) if target.workspace_id == workspace))
    );
}

fn badge(state: &mut ClientShellState, workspace: &str) -> String {
    let mut current_surface = surface();
    current_surface.projection_revision = state.snapshot.as_ref().unwrap().revision;
    state.set_pane_surface(current_surface);
    let frame = state.compose(100, 50).unwrap().to_ratatui_buffer().unwrap();
    let hit = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == workspace)
        .unwrap();
    (hit.rect.right() - 2..hit.rect.right())
        .map(|x| frame[(x, hit.rect.y)].symbol())
        .collect()
}

#[test]
fn command_space_hold_both_keys_and_release_reset_badges() {
    let mut state = command_state(10);
    assert!(state.host_keyboard_report_all_requested());
    assert_ne!(badge(&mut state, "ws_2"), "⌘2");
    for bytes in [b"\x1b[57444;9u".as_slice(), b"\x1b[57450;9u"] {
        let hold = state.handle_input_bytes(bytes);
        assert!(hold.requests.is_empty() && hold.actions.is_empty() && hold.repaint);
    }
    assert_eq!(badge(&mut state, "ws_2"), "⌘2");
    assert_ne!(badge(&mut state, "ws_10"), "⌘0");
    state.handle_input_bytes(b"\x1b[57444;9:3u");
    assert_eq!(badge(&mut state, "ws_2"), "⌘2");
    state.handle_input_bytes(b"\x1b[57450;1:3u");
    assert_ne!(badge(&mut state, "ws_2"), "⌘2");
}

#[test]
fn command_space_direct_jump_no_repeat_or_release_leak() {
    let mut state = command_state(10);
    state.handle_input_bytes(b"\x1b[57444;9u");
    assert_focus(&state.handle_input_bytes(b"\x1b[50;9u"), "ws_2");
    for bytes in [
        b"\x1b[50;9:2u".as_slice(),
        b"\x1b[57444;1:3u",
        b"\x1b[50;1:3u",
    ] {
        let event = state.handle_input_bytes(bytes);
        assert!(event.requests.is_empty() && event.actions.is_empty());
    }
    // A terminal can omit the standalone press while composing IME text.
    assert_focus(&state.handle_input_bytes(b"\x1b[57;9u"), "ws_9");
}

#[test]
fn command_space_order_frozen_during_hold_and_recaptured_after_release() {
    let mut state = command_state(3);
    state.handle_input_bytes(b"\x1b[57444;9u");
    let mut snapshot = (**state.snapshot.as_ref().unwrap()).clone();
    snapshot.revision += 1;
    snapshot.workspaces.swap(0, 1);
    state.set_snapshot(Box::new(snapshot));
    assert_eq!(badge(&mut state, "ws_1"), "⌘1");
    assert_focus(&state.handle_input_bytes(b"\x1b[49;9u"), "ws_1");
    state.handle_input_bytes(b"\x1b[49;9:3u\x1b[57444;1:3u\x1b[57444;9u");
    assert_eq!(badge(&mut state, "ws_2"), "⌘1");
    assert_focus(&state.handle_input_bytes(b"\x1b[49;9u"), "ws_2");
}

#[test]
fn command_space_deleted_target_does_not_retarget_next_space() {
    let mut state = command_state(3);
    state.handle_input_bytes(b"\x1b[57444;9u");
    let mut snapshot = (**state.snapshot.as_ref().unwrap()).clone();
    snapshot.revision += 1;
    snapshot.workspaces.remove(1);
    state.set_snapshot(Box::new(snapshot));
    let outcome = state.handle_input_bytes(b"\x1b[50;9u");
    assert!(outcome.requests.is_empty() && outcome.actions.is_empty());
    assert_eq!(badge(&mut state, "ws_3"), "⌘3");
}

#[test]
fn command_space_focus_loss_and_missing_release_clear_hints() {
    let mut state = command_state(2);
    state.handle_input_bytes(b"\x1b[57444;9u");
    let lost = state.handle_raw_events(vec![RawInputEvent::OuterFocusLost]);
    assert!(lost.repaint);
    assert_ne!(badge(&mut state, "ws_2"), "⌘2");
    state.handle_input_bytes(b"\x1b[57444;9u");
    state.handle_input_bytes(b"\x1b[120;1u");
    assert_ne!(badge(&mut state, "ws_2"), "⌘2");
}

#[test]
fn command_space_preserves_plain_text_ime_zero_letters_and_other_chords() {
    let mut state = command_state(2);
    for bytes in [
        b"hello".as_slice(),
        "日本語".as_bytes(),
        b"\x1b[48;9u",
        b"\x1b[97;9u",
        b"\x1b[50;13u",
        b"\x1b[50;1u",
    ] {
        let outcome = state.handle_input_bytes(bytes);
        assert!(outcome.actions.is_empty());
        assert!(!outcome.requests.is_empty(), "{bytes:?}");
    }
    for bytes in [b"\x1b[57441;2u".as_slice(), b"\x1b[57441;1:3u"] {
        let outcome = state.handle_input_bytes(bytes);
        assert!(outcome.requests.is_empty() && outcome.actions.is_empty());
    }
    state.set_pane_surface(surface_with_popup());
    let outcome = state.handle_input_bytes(b"\x1b[57444;9u\x1b[50;9u");
    assert!(outcome.requests.is_empty() && outcome.actions.is_empty());
}

#[test]
fn command_space_does_not_cross_endpoint_or_restarted_snapshot() {
    let mut state = command_state(2);
    state.handle_input_bytes(b"\x1b[57444;9u");
    let mut snapshot = (**state.snapshot.as_ref().unwrap()).clone();
    snapshot.boot_id = "replacement".into();
    state.set_snapshot(Box::new(snapshot));
    let outcome = state.handle_input_bytes(b"\x1b[50;9u");
    assert!(outcome.requests.is_empty() && outcome.actions.is_empty());
}

#[test]
fn command_space_sidebar_grouping_and_scroll_do_not_renumber() {
    let mut state = command_state(15);
    let mut snapshot = (**state.snapshot.as_ref().unwrap()).clone();
    for (index, linked) in [(0, false), (2, true)] {
        snapshot.workspaces[index].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: linked,
        });
    }
    state.set_snapshot(Box::new(snapshot));
    state.handle_input_bytes(b"\x1b[57444;9u");
    assert_focus(&state.handle_input_bytes(b"\x1b[50;9u"), "ws_3");
    state.handle_input_bytes(b"\x1b[50;9:3u");
    state.workspace_scroll = 2;
    assert_focus(&state.handle_input_bytes(b"\x1b[51;9u"), "ws_2");
    state.handle_input_bytes(b"\x1b[51;9:3u\x1b[57444;1:3u");
    state.sidebar_collapsed = true;
    state.handle_input_bytes(b"\x1b[57444;9u");
    assert_focus(&state.handle_input_bytes(b"\x1b[50;9u"), "ws_2");
}

#[test]
fn command_space_absent_digit_and_disabled_feature_are_safe() {
    let mut state = command_state(2);
    let absent = state.handle_input_bytes(b"\x1b[57;9u");
    assert!(absent.requests.is_empty() && absent.actions.is_empty());
    state.command_spaces.enabled = false;
    assert!(!state.host_keyboard_report_all_requested());
    let ordinary = state.handle_input_bytes(b"\x1b[50;9u");
    assert!(ordinary.actions.is_empty() && !ordinary.requests.is_empty());
}

#[test]
fn command_space_does_not_steal_a_forwarded_digit_repeat_lease() {
    let mut state = command_state(2);
    assert!(!state.handle_input_bytes(b"\x1b[50;1u").requests.is_empty());
    state.handle_input_bytes(b"\x1b[57444;9u");
    let repeated = state.handle_input_bytes(b"\x1b[50;9:2u");
    assert!(repeated.actions.is_empty() && !repeated.requests.is_empty());
    let released = state.handle_input_bytes(b"\x1b[50;9:3u");
    assert!(released.actions.is_empty() && !released.requests.is_empty());
}

#[test]
fn command_space_badges_leave_group_toggle_visible() {
    let mut state = command_state(3);
    let mut snapshot = (**state.snapshot.as_ref().unwrap()).clone();
    for (index, linked) in [(0, false), (2, true)] {
        snapshot.workspaces[index].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: linked,
        });
    }
    state.set_snapshot(Box::new(snapshot));
    let before = state.compose(100, 28).unwrap().to_ratatui_buffer().unwrap();
    let toggle = state
        .hits
        .workspaces
        .iter()
        .find_map(|hit| hit.group_toggle.as_ref())
        .unwrap()
        .0;
    state.handle_input_bytes(b"\x1b[57444;9u");
    let after = state.compose(100, 28).unwrap().to_ratatui_buffer().unwrap();
    assert_eq!(
        before[(toggle.x, toggle.y)].symbol(),
        after[(toggle.x, toggle.y)].symbol()
    );
    assert_eq!(after[(toggle.x - 2, toggle.y)].symbol(), "⌘");
    assert_eq!(after[(toggle.x - 1, toggle.y)].symbol(), "1");
}

#[test]
#[ignore = "non-gating Command badge render scaling profile"]
fn command_space_render_scale_profile() {
    for count in [1, 15] {
        let mut state = command_state(count);
        state.compose(100, 50).unwrap();
        for held in [false, true] {
            if held {
                state.handle_input_bytes(b"\x1b[57444;9u");
            }
            let started = std::time::Instant::now();
            for _ in 0..1000 {
                std::hint::black_box(state.compose(100, 50).unwrap());
            }
            eprintln!(
                "command badges: spaces={count} held={held} geometry=100x50 mean_us={:.2}",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
