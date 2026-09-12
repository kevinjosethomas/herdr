use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::schema::AgentStatus;
use crate::protocol::{ClientShellAgent, ClientShellSnapshot, PaneSurfaceFrame};

#[derive(Clone, Debug, Default)]
pub(super) struct EndpointAgentPresentation {
    boot_id: Option<String>,
    acknowledged: HashMap<String, u64>,
    dwell: HashMap<String, (u64, Instant)>,
}

impl EndpointAgentPresentation {
    pub(super) fn project_snapshot(&mut self, snapshot: &mut ClientShellSnapshot) {
        if self.boot_id.as_deref() != Some(snapshot.boot_id.as_str()) {
            self.boot_id = Some(snapshot.boot_id.clone());
            self.acknowledged.clear();
            self.dwell.clear();
            self.acknowledged.extend(
                snapshot
                    .agents
                    .iter()
                    .map(|agent| (agent.pane_id.clone(), agent.state_change_seq)),
            );
        }
        self.acknowledged.retain(|pane_id, _| {
            snapshot
                .agents
                .iter()
                .any(|agent| &agent.pane_id == pane_id)
        });
        for agent in &mut snapshot.agents {
            agent.agent_status = self.projected_status(agent);
        }
        project_aggregate_status(snapshot);
    }

    pub(super) fn reset_dwell(&mut self) {
        self.dwell.clear();
    }

    /// Only coherent, continuously visible completion evidence earns a read watermark.
    /// Each pane has its own sequence-bound timer; a new completion starts over.
    pub(super) fn observe_surface(
        &mut self,
        snapshot: &mut ClientShellSnapshot,
        surface: &PaneSurfaceFrame,
        outer_focused: Option<bool>,
        now: Instant,
    ) -> bool {
        if outer_focused == Some(false)
            || self.boot_id.as_deref() != Some(surface.boot_id.as_str())
            || snapshot.boot_id != surface.boot_id
            || snapshot.revision != surface.projection_revision
        {
            self.reset_dwell();
            return false;
        }
        self.dwell.retain(|pane_id, (sequence, _)| {
            surface.panes.iter().any(|pane| &pane.pane_id == pane_id)
                && snapshot.agents.iter().any(|agent| {
                    &agent.pane_id == pane_id
                        && agent.state_change_seq == *sequence
                        && agent.agent_status == AgentStatus::Done
                })
        });
        let mut changed = false;
        for pane in &surface.panes {
            let Some(agent) = snapshot.agents.iter().find(|agent| {
                agent.pane_id == pane.pane_id && agent.agent_status == AgentStatus::Done
            }) else {
                continue;
            };
            let (_, since) = self
                .dwell
                .entry(agent.pane_id.clone())
                .or_insert((agent.state_change_seq, now));
            if now.saturating_duration_since(*since) >= Duration::from_secs(5) {
                self.acknowledged
                    .insert(agent.pane_id.clone(), agent.state_change_seq);
                changed = true;
            }
        }
        if changed {
            for agent in &mut snapshot.agents {
                agent.agent_status = self.projected_status(agent);
            }
            project_aggregate_status(snapshot);
        }
        changed
    }

    pub(super) fn acknowledge_surface(
        &mut self,
        snapshot: &mut ClientShellSnapshot,
        surface: &PaneSurfaceFrame,
        outer_focused: Option<bool>,
    ) -> bool {
        if outer_focused == Some(false)
            || self.boot_id.as_deref() != Some(surface.boot_id.as_str())
            || snapshot.boot_id != surface.boot_id
            || snapshot.revision != surface.projection_revision
        {
            return false;
        }

        let mut changed = false;
        for pane in &surface.panes {
            let Some(agent) = snapshot.agents.iter().find(|agent| {
                agent.pane_id == pane.pane_id && agent.agent_status == AgentStatus::Done
            }) else {
                continue;
            };
            let acknowledged = self.acknowledged.entry(agent.pane_id.clone()).or_default();
            if *acknowledged < agent.state_change_seq {
                *acknowledged = agent.state_change_seq;
                changed = true;
            }
        }
        if changed {
            for agent in &mut snapshot.agents {
                agent.agent_status = self.projected_status(agent);
            }
            project_aggregate_status(snapshot);
        }
        changed
    }

    fn projected_status(&self, agent: &ClientShellAgent) -> AgentStatus {
        match agent.agent_status {
            AgentStatus::Idle | AgentStatus::Done => {
                if self
                    .acknowledged
                    .get(&agent.pane_id)
                    .is_some_and(|sequence| *sequence >= agent.state_change_seq)
                {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Done
                }
            }
            status => status,
        }
    }
}

fn project_aggregate_status(snapshot: &mut ClientShellSnapshot) {
    for tab in &mut snapshot.tabs {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.tab_id == tab.tab_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| super::status_priority(*status))
        {
            tab.agent_status = status;
        }
    }
    for workspace in &mut snapshot.workspaces {
        if let Some(status) = snapshot
            .agents
            .iter()
            .filter(|agent| agent.workspace_id == workspace.workspace_id)
            .map(|agent| agent.agent_status)
            .max_by_key(|status| super::status_priority(*status))
        {
            workspace.agent_status = status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FrameData, PaneSurfacePane, SurfaceRect};

    fn agent(status: AgentStatus, sequence: u64) -> ClientShellAgent {
        ClientShellAgent {
            pane_id: "agent-pane".into(),
            workspace_id: "workspace".into(),
            tab_id: "tab".into(),
            name: None,
            display_agent: None,
            agent: None,
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: status,
            state_change_seq: sequence,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: true,
        }
    }

    fn snapshot(status: AgentStatus, sequence: u64, revision: u64) -> ClientShellSnapshot {
        let mut snapshot = crate::client::shell::tests::snapshot();
        snapshot.boot_id = "endpoint-boot".into();
        snapshot.revision = revision;
        snapshot.agents = vec![agent(status, sequence)];
        snapshot
    }

    fn surface(revision: u64) -> PaneSurfaceFrame {
        let rect = SurfaceRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        PaneSurfaceFrame {
            boot_id: "endpoint-boot".into(),
            projection_revision: revision,
            surface_revision: 1,
            frame: FrameData {
                cells: Vec::new(),
                width: 0,
                height: 0,
                cursor: None,
                hyperlinks: Vec::new(),
                graphics: Vec::new(),
            },
            panes: vec![PaneSurfacePane {
                pane_id: "agent-pane".into(),
                content_revision: 1,
                rect,
                inner_rect: rect,
                scrollbar_rect: None,
                scroll: None,
                focused: true,
                mouse_reporting: false,
                sgr_pixel_mouse: false,
                alternate_screen_active: false,
                pixel_width: 0,
                pixel_height: 0,
            }],
            splits: Vec::new(),
            popup: None,
            graphics: Default::default(),
        }
    }

    #[test]
    fn first_snapshot_establishes_an_idle_baseline_without_server_seen_authority() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut snapshot = snapshot(AgentStatus::Done, 4, 1);

        presentation.project_snapshot(&mut snapshot);

        assert_eq!(snapshot.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn unpresented_working_completion_projects_done() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);

        presentation.project_snapshot(&mut completed);

        assert_eq!(completed.agents[0].agent_status, AgentStatus::Done);
    }

    #[test]
    fn coherent_presented_surface_acknowledges_completion() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);

        assert!(presentation.acknowledge_surface(&mut completed, &surface(2), Some(true)));
        assert_eq!(completed.agents[0].agent_status, AgentStatus::Idle);
    }

    #[test]
    fn clients_acknowledge_the_same_endpoint_completion_independently() {
        let mut viewing_client = EndpointAgentPresentation::default();
        let mut background_client = EndpointAgentPresentation::default();
        let mut initial_for_viewer = snapshot(AgentStatus::Working, 4, 1);
        let mut initial_for_background = initial_for_viewer.clone();
        viewing_client.project_snapshot(&mut initial_for_viewer);
        background_client.project_snapshot(&mut initial_for_background);
        let mut completed_for_viewer = snapshot(AgentStatus::Idle, 5, 2);
        let mut completed_for_background = completed_for_viewer.clone();
        viewing_client.project_snapshot(&mut completed_for_viewer);
        background_client.project_snapshot(&mut completed_for_background);

        assert!(viewing_client.acknowledge_surface(
            &mut completed_for_viewer,
            &surface(2),
            Some(true)
        ));

        assert_eq!(
            completed_for_viewer.agents[0].agent_status,
            AgentStatus::Idle
        );
        assert_eq!(
            completed_for_background.agents[0].agent_status,
            AgentStatus::Done
        );
    }

    #[test]
    fn stale_or_unfocused_surface_does_not_acknowledge_completion() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);

        assert!(!presentation.acknowledge_surface(&mut completed, &surface(1), Some(true)));
        assert!(!presentation.acknowledge_surface(&mut completed, &surface(2), Some(false)));
        assert_eq!(completed.agents[0].agent_status, AgentStatus::Done);
    }
    #[test]
    fn completion_requires_five_continuous_seconds_and_new_sequence_restarts() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);
        let start = Instant::now();
        assert!(!presentation.observe_surface(&mut completed, &surface(2), Some(true), start));
        assert!(!presentation.observe_surface(
            &mut completed,
            &surface(2),
            Some(true),
            start + Duration::from_millis(4999)
        ));
        assert!(presentation.observe_surface(
            &mut completed,
            &surface(2),
            Some(true),
            start + Duration::from_secs(5)
        ));
        assert_eq!(completed.agents[0].agent_status, AgentStatus::Idle);

        let mut next = snapshot(AgentStatus::Idle, 7, 3);
        presentation.project_snapshot(&mut next);
        assert!(!presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(6)
        ));
        assert!(!presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(false),
            start + Duration::from_secs(10)
        ));
        assert!(!presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(20)
        ));
        assert!(presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(25)
        ));
    }

    #[test]
    fn passing_spaces_and_replaced_completion_cannot_inherit_dwell() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 4, 1);
        presentation.project_snapshot(&mut initial);
        let mut completed = snapshot(AgentStatus::Idle, 5, 2);
        presentation.project_snapshot(&mut completed);
        let start = Instant::now();
        assert!(!presentation.observe_surface(&mut completed, &surface(2), Some(true), start));
        let mut other = surface(2);
        other.panes.clear();
        assert!(!presentation.observe_surface(
            &mut completed,
            &other,
            Some(true),
            start + Duration::from_secs(4)
        ));
        assert!(!presentation.observe_surface(
            &mut completed,
            &surface(2),
            Some(true),
            start + Duration::from_secs(10)
        ));
        let mut next = snapshot(AgentStatus::Idle, 7, 3);
        presentation.project_snapshot(&mut next);
        assert!(!presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(14)
        ));
        assert!(!presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(15)
        ));
        assert!(presentation.observe_surface(
            &mut next,
            &surface(3),
            Some(true),
            start + Duration::from_secs(19)
        ));
    }

    #[test]
    fn manual_ack_never_records_working_or_blocked_sequence() {
        for status in [AgentStatus::Working, AgentStatus::Blocked] {
            let mut presentation = EndpointAgentPresentation::default();
            let mut initial = snapshot(AgentStatus::Idle, 1, 1);
            presentation.project_snapshot(&mut initial);
            let mut current = snapshot(status, 2, 2);
            presentation.project_snapshot(&mut current);
            assert!(!presentation.acknowledge_surface(&mut current, &surface(2), Some(true)));
            assert_eq!(current.agents[0].agent_status, status);
            assert_eq!(presentation.acknowledged["agent-pane"], 1);
        }
    }
    #[test]
    fn split_panes_earn_independent_watermarks_and_reset_on_disconnect() {
        let mut presentation = EndpointAgentPresentation::default();
        let mut initial = snapshot(AgentStatus::Working, 1, 1);
        let mut second = agent(AgentStatus::Working, 1);
        second.pane_id = "second".into();
        initial.agents.push(second);
        presentation.project_snapshot(&mut initial);
        let mut completed = initial.clone();
        completed.revision = 2;
        for agent in &mut completed.agents {
            agent.agent_status = AgentStatus::Idle;
            agent.state_change_seq = 2;
        }
        presentation.project_snapshot(&mut completed);
        let start = Instant::now();
        assert!(!presentation.observe_surface(&mut completed, &surface(2), Some(true), start));
        let mut split = surface(2);
        let mut pane = split.panes[0].clone();
        pane.pane_id = "second".into();
        split.panes.push(pane);
        assert!(!presentation.observe_surface(
            &mut completed,
            &split,
            Some(true),
            start + Duration::from_secs(3)
        ));
        assert!(presentation.observe_surface(
            &mut completed,
            &split,
            Some(true),
            start + Duration::from_secs(5)
        ));
        assert_eq!(completed.agents[1].agent_status, AgentStatus::Done);
        presentation.reset_dwell();
        assert!(!presentation.observe_surface(
            &mut completed,
            &split,
            Some(true),
            start + Duration::from_secs(8)
        ));
        assert!(presentation.observe_surface(
            &mut completed,
            &split,
            Some(true),
            start + Duration::from_secs(13)
        ));
        assert_eq!(completed.agents[1].agent_status, AgentStatus::Idle);
    }
}
