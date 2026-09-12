//! Client-local Command hold hints. No server state or protocol fields.
use super::*;
use crossterm::event::{KeyEventKind, KeyModifiers, ModifierKeyCode};

pub(super) struct CommandSpaces {
    pub(super) enabled: bool,
    held: u8,
    consumed_digits: u16,
    targets: Vec<WorkspaceNavigationTarget>,
}

impl CommandSpaces {
    pub(super) fn new() -> Self {
        Self {
            enabled: crate::platform::capabilities().command_space_shortcuts,
            held: 0,
            consumed_digits: 0,
            targets: Vec::new(),
        }
    }
}

impl ClientShellState {
    pub(super) fn clear_command_spaces(&mut self, outcome: &mut ClientShellInput) {
        outcome.repaint |= self.command_spaces.held != 0;
        self.command_spaces.held = 0;
        self.command_spaces.targets.clear();
        self.command_spaces.consumed_digits = 0;
    }

    fn capture_command_spaces(&mut self) {
        self.command_spaces.targets = self.snapshot.as_deref().map_or_else(Vec::new, |snapshot| {
            // The compact sidebar uses raw snapshot order; the expanded sidebar
            // applies worktree grouping. Never use viewport hits (scroll dependent).
            let indices: Vec<_> = if self.sidebar_collapsed {
                (0..snapshot.workspaces.len()).take(9).collect()
            } else {
                self.navigation_workspace_entries(snapshot)
                    .into_iter()
                    .take(9)
                    .map(|entry| entry.index)
                    .collect()
            };
            indices
                .into_iter()
                .filter_map(|index| {
                    self.navigation_target(
                        &self.active_endpoint_id,
                        &snapshot.workspaces[index].workspace_id,
                    )
                })
                .collect()
        });
    }

    pub(super) fn handle_command_space_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        if !self.command_spaces.enabled {
            return false;
        }
        if let KeyCode::Modifier(modifier) = key.code {
            let side = match modifier {
                ModifierKeyCode::LeftSuper => 1,
                ModifierKeyCode::RightSuper => 2,
                _ => return true, // Never send standalone modifiers to panes/popups.
            };
            if key.kind == KeyEventKind::Release {
                self.command_spaces.held &= !side;
                if self.command_spaces.held == 0 {
                    self.command_spaces.targets.clear();
                }
            } else {
                if self.command_spaces.held == 0 {
                    self.capture_command_spaces();
                }
                self.command_spaces.held |= side;
            }
            outcome.repaint = true;
            return true;
        }
        // Recover from a lost modifier release (e.g. an OS menu consumed it).
        if self.command_spaces.held != 0 && !key.modifiers.contains(KeyModifiers::SUPER) {
            self.command_spaces.held = 0;
            self.command_spaces.targets.clear();
            outcome.repaint = true;
        }
        let KeyCode::Char(digit @ '1'..='9') = key.code else {
            return false;
        };
        let index = (digit as u8 - b'1') as usize;
        let bit = 1 << index;
        if key.kind == KeyEventKind::Release {
            let consumed = self.command_spaces.consumed_digits & bit != 0;
            self.command_spaces.consumed_digits &= !bit;
            return consumed;
        }
        if self.command_spaces.consumed_digits & bit != 0 {
            if key.kind == KeyEventKind::Repeat || key.modifiers.contains(KeyModifiers::SUPER) {
                return true;
            }
            self.command_spaces.consumed_digits &= !bit;
        }
        if key.modifiers != KeyModifiers::SUPER {
            return false;
        }
        // A plain digit may already have a forwarded lease when Command is
        // pressed during its repeat. Do not steal that repeat or its release.
        if key.kind != KeyEventKind::Press {
            return false;
        }
        self.command_spaces.consumed_digits |= bit;
        if self.overlay.is_some()
            || self.popup_pending
            || self.popup_terminal_id.is_some()
            || self.copy_operation_in_flight
        {
            return true;
        }
        // A Cmd+digit can arrive without a standalone press during IME preedit.
        // Still support the direct jump, without pretending a hold was observed.
        if self.command_spaces.held == 0 {
            self.capture_command_spaces();
        }
        let target = self.command_spaces.targets.get(index).cloned();
        if self.command_spaces.held == 0 {
            self.command_spaces.targets.clear();
        }
        if let Some(target) = target.filter(|target| {
            target.endpoint_id == self.active_endpoint_id && self.navigation_target_valid(target)
        }) {
            if self.focus_or_activate(
                target.endpoint_id,
                ClientEndpointFocusTarget::Workspace(target.workspace_id),
                outcome,
            ) {
                self.mode = ClientShellMode::Terminal;
                self.navigate_workspace_id = None;
            }
            outcome.repaint = true;
        }
        true
    }

    pub(super) fn render_command_space_badges(&self, buffer: &mut Buffer) {
        if self.command_spaces.held == 0 {
            return;
        }
        // Validate the frozen targets once, outside the visible-row loop.
        // A restarted endpoint must not advertise an unusable old shortcut.
        let valid: [bool; 9] = std::array::from_fn(|index| {
            self.command_spaces
                .targets
                .get(index)
                .is_some_and(|target| self.navigation_target_valid(target))
        });
        // At most nine targets: bounded work per visible sidebar row. No PTY locks.
        for hit in &self.hits.workspaces {
            if hit.endpoint_id != self.active_endpoint_id || hit.rect.width < 2 {
                continue;
            }
            if let Some(index) =
                self.command_spaces
                    .targets
                    .iter()
                    .enumerate()
                    .position(|(index, target)| {
                        valid[index] && target.matches(&hit.endpoint_id, &hit.workspace_id)
                    })
            {
                // Leave worktree expansion controls visible and clickable.
                let right = hit
                    .group_toggle
                    .as_ref()
                    .map_or(hit.rect.right(), |(rect, _)| rect.x);
                if right < hit.rect.x.saturating_add(2) {
                    continue;
                }
                buffer.set_stringn(
                    right.saturating_sub(2),
                    hit.rect.y,
                    format!("⌘{}", index + 1),
                    2,
                    Style::default()
                        .fg(self.config.palette.accent)
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
}
