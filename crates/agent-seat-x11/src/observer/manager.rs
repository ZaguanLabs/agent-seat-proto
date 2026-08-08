//! Freshness-checked EWMH management over the observer's X11 connection.

use std::thread;
use std::time::{Duration, Instant};

use agent_seat_proto::{
    ClientAction, ClientGeometryRequest, ClientId, ClientState, ClientStateRequest,
    ClientWorkspaceRequest, ErrorCode, ManagementReply, Observation as ManagementObservation, Rect,
    Retry, StateAction, TargetRequest, WorkspaceId, WorkspaceRequest,
};
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask};

use super::{
    Failure, MAX_CLIENT_ATOMS, MAX_ROOT_ATOMS, Model, Observer, client_frame_extents, property32,
};

const MANAGEMENT_TIMEOUT: Duration = Duration::from_secs(1);
const PAGER_SOURCE: u32 = 2;
const STATIC_GRAVITY: u32 = 10;

pub(crate) enum Operation {
    Activate(TargetRequest),
    Close(TargetRequest),
    WorkspaceSwitch(WorkspaceRequest),
    ClientWorkspace(ClientWorkspaceRequest),
    State(ClientStateRequest),
    Geometry(ClientGeometryRequest),
}

impl Observer {
    pub(crate) fn manage(&mut self, operation: Operation) -> Result<ManagementReply, Failure> {
        self.connection
            .grab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server grab"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot acquire an X11 server grab"))?;

        let sent = self
            .refresh()
            .and_then(|()| self.prepare(operation))
            .and_then(|prepared| {
                self.connection
                    .send_event(
                        false,
                        self.root,
                        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                        prepared.event,
                    )
                    .map_err(|_| Failure::unavailable("cannot send an EWMH request"))?
                    .check()
                    .map_err(|_| Failure::unavailable("the X server refused an EWMH request"))?;
                Ok(prepared.desired)
            });
        let ungrab = self
            .connection
            .ungrab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server ungrab"))
            .and_then(|cookie| {
                cookie
                    .check()
                    .map_err(|_| Failure::unavailable("cannot release the X11 server grab"))
            });
        let flush = self
            .connection
            .flush()
            .map_err(|_| Failure::unavailable("cannot flush an EWMH request"));
        let desired = sent?;
        ungrab?;
        flush?;
        Ok(self.observe_terminal(desired))
    }

    fn prepare(&self, operation: Operation) -> Result<Prepared, Failure> {
        let supported = property32(
            &self.connection,
            self.root,
            self.atoms.supported,
            AtomEnum::ATOM.into(),
            MAX_ROOT_ATOMS,
        )?
        .unwrap_or_default();
        match operation {
            Operation::Activate(request) => {
                let target = self.target(request)?;
                require_action(&target, ClientAction::Activate)?;
                require_supported(&supported, self.atoms.active_window)?;
                let active = self
                    .model()?
                    .active
                    .and_then(|id| self.record(id))
                    .map_or(0, |record| record.xid);
                Ok(Prepared {
                    event: ClientMessageEvent::new(
                        32,
                        target.xid,
                        self.atoms.active_window,
                        [PAGER_SOURCE, 0, active, 0, 0],
                    ),
                    desired: Desired::Active(target.id),
                })
            }
            Operation::Close(request) => {
                let target = self.target(request)?;
                require_action(&target, ClientAction::Close)?;
                require_supported(&supported, self.atoms.close_window)?;
                Ok(Prepared {
                    event: ClientMessageEvent::new(
                        32,
                        target.xid,
                        self.atoms.close_window,
                        [0, PAGER_SOURCE, 0, 0, 0],
                    ),
                    desired: Desired::Closed(target.id),
                })
            }
            Operation::WorkspaceSwitch(request) => {
                if request.sequence != self.sequence() {
                    return Err(stale_sequence(self.sequence()));
                }
                self.require_workspace(request.workspace)?;
                require_supported(&supported, self.atoms.current_desktop)?;
                Ok(Prepared {
                    event: ClientMessageEvent::new(
                        32,
                        self.root,
                        self.atoms.current_desktop,
                        [u32::from(request.workspace.get()), 0, 0, 0, 0],
                    ),
                    desired: Desired::Workspace(request.workspace),
                })
            }
            Operation::ClientWorkspace(request) => {
                let target = self.target(request.target)?;
                self.require_workspace(request.workspace)?;
                require_action(&target, ClientAction::ChangeWorkspace)?;
                require_supported(&supported, self.atoms.wm_desktop)?;
                Ok(Prepared {
                    event: ClientMessageEvent::new(
                        32,
                        target.xid,
                        self.atoms.wm_desktop,
                        [u32::from(request.workspace.get()), PAGER_SOURCE, 0, 0, 0],
                    ),
                    desired: Desired::ClientWorkspace(target.id, request.workspace),
                })
            }
            Operation::State(request) => self.prepare_state(request, &supported),
            Operation::Geometry(request) => self.prepare_geometry(request, &supported),
        }
    }

    fn prepare_state(
        &self,
        request: ClientStateRequest,
        supported: &[u32],
    ) -> Result<Prepared, Failure> {
        let target = self.target(request.target)?;
        let (state_atom, action_atom) = match request.state {
            ClientState::Above => (self.atoms.state_above, Some(self.atoms.action_above)),
            ClientState::Below => (self.atoms.state_below, Some(self.atoms.action_below)),
            ClientState::Fullscreen => (
                self.atoms.state_fullscreen,
                Some(self.atoms.action_fullscreen),
            ),
            ClientState::Hidden => {
                return Err(Failure::unsupported(
                    "EWMH defines hidden as window-manager state, not a client request",
                ));
            }
            ClientState::MaximizedHorizontal => (
                self.atoms.state_maximized_horz,
                Some(self.atoms.action_maximize_horz),
            ),
            ClientState::MaximizedVertical => (
                self.atoms.state_maximized_vert,
                Some(self.atoms.action_maximize_vert),
            ),
            ClientState::DemandsAttention => (self.atoms.state_demands_attention, None),
            ClientState::Sticky => (self.atoms.state_sticky, Some(self.atoms.action_stick)),
            ClientState::Shaded => (self.atoms.state_shaded, Some(self.atoms.action_shade)),
        };
        require_supported(supported, self.atoms.wm_state)?;
        require_supported(supported, state_atom)?;
        if let Some(action_atom) = action_atom {
            let allowed = self.allowed_actions(target.xid)?;
            if !allowed.contains(&action_atom) {
                return Err(Failure::unsupported(
                    "the target does not advertise the requested state action",
                ));
            }
        }
        let present = target.states.contains(&request.state);
        let desired_present = match request.action {
            StateAction::Add => true,
            StateAction::Remove => false,
            StateAction::Toggle => !present,
        };
        let action = match request.action {
            StateAction::Remove => 0,
            StateAction::Add => 1,
            StateAction::Toggle => 2,
        };
        Ok(Prepared {
            event: ClientMessageEvent::new(
                32,
                target.xid,
                self.atoms.wm_state,
                [action, state_atom, 0, PAGER_SOURCE, 0],
            ),
            desired: Desired::State(target.id, request.state, desired_present),
        })
    }

    fn prepare_geometry(
        &self,
        request: ClientGeometryRequest,
        supported: &[u32],
    ) -> Result<Prepared, Failure> {
        let target = self.target(request.target)?;
        require_supported(supported, self.atoms.moveresize_window)?;
        let current = target
            .frame
            .ok_or_else(|| Failure::unsupported("the target has no observable frame geometry"))?;
        let move_x = request.frame.x != current.x;
        let move_y = request.frame.y != current.y;
        let resize_width = request.frame.width != current.width;
        let resize_height = request.frame.height != current.height;
        let allowed = self.allowed_actions(target.xid)?;
        if (move_x || move_y) && !allowed.contains(&self.atoms.action_move) {
            return Err(Failure::unsupported(
                "the target does not advertise movement",
            ));
        }
        if (resize_width || resize_height) && !allowed.contains(&self.atoms.action_resize) {
            return Err(Failure::unsupported(
                "the target does not advertise resizing",
            ));
        }
        if !(move_x
            || move_y
            || resize_width
            || resize_height
            || target.actions.contains(&ClientAction::ChangeGeometry))
        {
            return Err(Failure::unsupported(
                "the target does not advertise geometry changes",
            ));
        }
        let extents = client_frame_extents(&self.connection, target.xid, &self.atoms);
        let client_x = request
            .frame
            .x
            .checked_add(i32::try_from(extents[0]).map_err(|_| invalid_geometry())?)
            .ok_or_else(invalid_geometry)?;
        let client_y = request
            .frame
            .y
            .checked_add(i32::try_from(extents[2]).map_err(|_| invalid_geometry())?)
            .ok_or_else(invalid_geometry)?;
        let horizontal_extents = extents[0]
            .checked_add(extents[1])
            .ok_or_else(invalid_geometry)?;
        let vertical_extents = extents[2]
            .checked_add(extents[3])
            .ok_or_else(invalid_geometry)?;
        let client_width = request
            .frame
            .width
            .checked_sub(horizontal_extents)
            .filter(|width| *width != 0)
            .ok_or_else(invalid_geometry)?;
        let client_height = request
            .frame
            .height
            .checked_sub(vertical_extents)
            .filter(|height| *height != 0)
            .ok_or_else(invalid_geometry)?;
        let mut flags = STATIC_GRAVITY | (PAGER_SOURCE << 12);
        flags |= u32::from(move_x) << 8;
        flags |= u32::from(move_y) << 9;
        flags |= u32::from(resize_width) << 10;
        flags |= u32::from(resize_height) << 11;
        Ok(Prepared {
            event: ClientMessageEvent::new(
                32,
                target.xid,
                self.atoms.moveresize_window,
                [
                    flags,
                    u32::from_ne_bytes(client_x.to_ne_bytes()),
                    u32::from_ne_bytes(client_y.to_ne_bytes()),
                    client_width,
                    client_height,
                ],
            ),
            desired: Desired::Geometry(target.id, request.frame),
        })
    }

    fn observe_terminal(&mut self, desired: Desired) -> ManagementReply {
        let deadline = Instant::now() + MANAGEMENT_TIMEOUT;
        loop {
            if self.refresh().is_ok() {
                if desired.observed(self) {
                    return ManagementReply {
                        observation: ManagementObservation::Observed,
                        sequence: self.sequence(),
                    };
                }
                if desired
                    .target()
                    .is_some_and(|target| self.record(target).is_none())
                {
                    return ManagementReply {
                        observation: if matches!(desired, Desired::Closed(_)) {
                            ManagementObservation::Observed
                        } else {
                            ManagementObservation::TargetGone
                        },
                        sequence: self.sequence(),
                    };
                }
            }
            if Instant::now() >= deadline {
                return ManagementReply {
                    observation: ManagementObservation::TimedOut,
                    sequence: self.sequence(),
                };
            }
            thread::sleep(super::SAMPLE_INTERVAL);
        }
    }

    fn target(&self, request: TargetRequest) -> Result<Target, Failure> {
        let Some(record) = self.record(request.client) else {
            return Err(Failure {
                code: ErrorCode::NoSuchClient,
                retry: Retry::Reobserve,
                message: "the target is missing or outside the configured scope",
                current_generation: None,
                current_sequence: Some(self.sequence()),
            });
        };
        if record.descriptor.generation != request.generation {
            return Err(Failure {
                code: ErrorCode::Stale,
                retry: Retry::Reobserve,
                message: "the target generation changed before send",
                current_generation: Some(record.descriptor.generation),
                current_sequence: Some(self.sequence()),
            });
        }
        Ok(Target {
            id: record.descriptor.id,
            xid: record.xid,
            frame: record.descriptor.frame,
            states: record.descriptor.states.to_vec(),
            actions: record.descriptor.actions.to_vec(),
        })
    }

    fn require_workspace(&self, workspace: WorkspaceId) -> Result<(), Failure> {
        if self
            .model()?
            .workspaces
            .iter()
            .any(|candidate| candidate.id == workspace)
        {
            Ok(())
        } else {
            Err(Failure {
                code: ErrorCode::InvalidArgument,
                retry: Retry::Never,
                message: "the workspace is outside the advertised range",
                current_generation: None,
                current_sequence: Some(self.sequence()),
            })
        }
    }

    fn allowed_actions(&self, xid: u32) -> Result<Vec<u32>, Failure> {
        property32(
            &self.connection,
            xid,
            self.atoms.wm_allowed_actions,
            AtomEnum::ATOM.into(),
            MAX_CLIENT_ATOMS,
        )
        .map(Option::unwrap_or_default)
    }

    fn model(&self) -> Result<&Model, Failure> {
        self.model
            .as_ref()
            .ok_or_else(|| Failure::internal("observer has no current model"))
    }

    fn record(&self, id: ClientId) -> Option<&super::ClientRecord> {
        self.model
            .as_ref()?
            .clients
            .iter()
            .find(|record| record.descriptor.id == id)
    }
}

struct Target {
    id: ClientId,
    xid: u32,
    frame: Option<Rect>,
    states: Vec<ClientState>,
    actions: Vec<ClientAction>,
}

struct Prepared {
    event: ClientMessageEvent,
    desired: Desired,
}

enum Desired {
    Active(ClientId),
    Closed(ClientId),
    Workspace(WorkspaceId),
    ClientWorkspace(ClientId, WorkspaceId),
    State(ClientId, ClientState, bool),
    Geometry(ClientId, Rect),
}

impl Desired {
    fn target(&self) -> Option<ClientId> {
        match *self {
            Self::Active(id)
            | Self::Closed(id)
            | Self::ClientWorkspace(id, _)
            | Self::State(id, _, _)
            | Self::Geometry(id, _) => Some(id),
            Self::Workspace(_) => None,
        }
    }

    fn observed(&self, observer: &Observer) -> bool {
        match *self {
            Self::Active(id) => observer
                .model
                .as_ref()
                .is_some_and(|model| model.active == Some(id)),
            Self::Closed(id) => observer.record(id).is_none(),
            Self::Workspace(workspace) => observer
                .model
                .as_ref()
                .is_some_and(|model| model.current_workspace == workspace),
            Self::ClientWorkspace(id, workspace) => observer
                .record(id)
                .is_some_and(|record| record.descriptor.workspace == Some(workspace)),
            Self::State(id, state, present) => observer
                .record(id)
                .is_some_and(|record| record.descriptor.states.contains(&state) == present),
            Self::Geometry(id, frame) => observer
                .record(id)
                .is_some_and(|record| record.descriptor.frame == Some(frame)),
        }
    }
}

fn require_action(target: &Target, action: ClientAction) -> Result<(), Failure> {
    if target.actions.contains(&action) {
        Ok(())
    } else {
        Err(Failure::unsupported(
            "the target does not advertise the requested operation",
        ))
    }
}

fn require_supported(supported: &[u32], atom: u32) -> Result<(), Failure> {
    if supported.contains(&atom) {
        Ok(())
    } else {
        Err(Failure::unsupported(
            "the window manager does not advertise the requested operation",
        ))
    }
}

fn stale_sequence(sequence: agent_seat_proto::Sequence) -> Failure {
    Failure {
        code: ErrorCode::Stale,
        retry: Retry::Reobserve,
        message: "the desktop sequence changed before send",
        current_generation: None,
        current_sequence: Some(sequence),
    }
}

const fn invalid_geometry() -> Failure {
    Failure {
        code: ErrorCode::InvalidArgument,
        retry: Retry::Never,
        message: "frame geometry cannot contain the target's decoration extents",
        current_generation: None,
        current_sequence: None,
    }
}
