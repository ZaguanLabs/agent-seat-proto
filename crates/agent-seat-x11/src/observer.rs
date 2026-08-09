//! Bounded per-session EWMH snapshots and convergent event diffs.

mod input;
mod keyboard;
mod manager;

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::thread;
use std::time::{Duration, Instant};

use agent_seat_proto::{
    BoundedList, ClientAction, ClientDescriptor, ClientId, ClientState, DesktopSnapshot, ErrorCode,
    Event, EventBatch, EventEnvelope, EventKind, Generation, MAX_CLIENTS, MAX_EVENTS,
    MAX_TITLE_BYTES, MAX_WORKSPACE_NAME_BYTES, MAX_WORKSPACES, Rect, Retry, Sequence, Subscription,
    Title, WorkspaceDescriptor, WorkspaceId, WorkspaceName,
};
use x11rb::NONE;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

use crate::config::ClientScope;

pub(crate) use manager::Operation;

const MAX_ROOT_ATOMS: usize = 256;
const MAX_CLIENT_ATOMS: usize = 64;
const MAX_STARTUP_ID_BYTES: usize = 256;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) struct Observer {
    connection: RustConnection,
    root: u32,
    atoms: Atoms,
    scope: ClientScope,
    show_titles: bool,
    model: Option<Model>,
    next_client: u64,
    sequence: u64,
    subscribed: bool,
    subscribed_kinds: Vec<EventKind>,
    events: VecDeque<EventEnvelope>,
    retained_after: u64,
    resync_required: bool,
}

impl Observer {
    pub(crate) fn connect(scope: ClientScope, show_titles: bool) -> Result<Self, Failure> {
        let (connection, screen) = x11rb::connect(None)
            .map_err(|_| Failure::unavailable("cannot connect an X11 observer"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| Failure::unavailable("selected X11 screen is absent"))?
            .root;
        let atoms = Atoms::intern(&connection)?;
        Ok(Self {
            connection,
            root,
            atoms,
            scope,
            show_titles,
            model: None,
            next_client: 1,
            sequence: 0,
            subscribed: false,
            subscribed_kinds: Vec::new(),
            events: VecDeque::with_capacity(MAX_EVENTS),
            retained_after: 0,
            resync_required: false,
        })
    }

    pub(crate) const fn sequence(&self) -> Sequence {
        Sequence::new(self.sequence)
    }

    pub(crate) fn launch_baseline(&mut self) -> Result<HashSet<u32>, Failure> {
        self.refresh()?;
        Ok(self
            .model
            .as_ref()
            .map(|model| model.clients.iter().map(|client| client.xid).collect())
            .unwrap_or_default())
    }

    pub(crate) fn correlate_launch(
        &mut self,
        baseline: &HashSet<u32>,
        startup_id: &str,
        wait: Duration,
    ) -> Result<Option<ClientId>, Failure> {
        let deadline = Instant::now() + wait;
        loop {
            self.refresh()?;
            if let Some(client) = self.model.as_ref().and_then(|model| {
                model.clients.iter().find(|client| {
                    !baseline.contains(&client.xid)
                        && client_startup_id_matches(
                            &self.connection,
                            client.xid,
                            &self.atoms,
                            startup_id.as_bytes(),
                        )
                })
            }) {
                return Ok(Some(client.descriptor.id));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(SAMPLE_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
        }
    }

    pub(crate) fn snapshot(&mut self) -> Result<DesktopSnapshot, Failure> {
        self.refresh()?;
        if self.resync_required {
            self.events.clear();
            self.retained_after = self.sequence;
            self.resync_required = false;
        }
        self.current_snapshot()
    }

    pub(crate) fn subscribe(&mut self, kinds: &[EventKind]) -> Result<Subscription, Failure> {
        self.refresh()?;
        self.subscribed = true;
        self.subscribed_kinds.clear();
        self.subscribed_kinds.extend_from_slice(kinds);
        self.events.clear();
        self.retained_after = self.sequence;
        self.resync_required = false;
        Ok(Subscription {
            cursor: self.sequence(),
        })
    }

    pub(crate) fn poll(
        &mut self,
        after: Sequence,
        limit: u16,
        wait_ms: u32,
    ) -> Result<EventBatch, Failure> {
        if !self.subscribed {
            return Err(Failure::invalid(
                "events.subscribe is required before polling",
            ));
        }
        let after = after.get();
        if after < self.retained_after || after > self.sequence {
            return Err(Failure::resync(self.sequence));
        }
        while self
            .events
            .front()
            .is_some_and(|event| event.sequence.get() <= after)
        {
            if let Some(event) = self.events.pop_front() {
                self.retained_after = event.sequence.get();
            }
        }

        let deadline = Instant::now() + Duration::from_millis(u64::from(wait_ms));
        loop {
            self.refresh()?;
            if self.resync_required {
                return Err(Failure::resync(self.sequence));
            }
            if self
                .events
                .front()
                .is_some_and(|event| event.sequence.get() > after)
                || Instant::now() >= deadline
            {
                break;
            }
            thread::sleep(SAMPLE_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
        }

        let events = self
            .events
            .iter()
            .filter(|event| event.sequence.get() > after)
            .take(usize::from(limit))
            .cloned()
            .collect::<Vec<_>>();
        let cursor = events
            .last()
            .map_or(self.sequence, |event| event.sequence.get());
        Ok(EventBatch {
            events: BoundedList::new(events)
                .map_err(|_| Failure::internal("event page exceeded its public bound"))?,
            cursor: Sequence::new(cursor),
        })
    }

    pub(super) fn refresh(&mut self) -> Result<(), Failure> {
        let raw = scan(
            &self.connection,
            self.root,
            &self.atoms,
            self.scope,
            self.show_titles,
        )?;
        let Some(previous) = self.model.take() else {
            self.model = Some(self.initial_model(raw)?);
            return Ok(());
        };
        self.model = Some(self.diff_model(previous, raw)?);
        Ok(())
    }

    fn initial_model(&mut self, raw: RawDesktop) -> Result<Model, Failure> {
        let mut clients = Vec::with_capacity(raw.clients.len());
        for client in raw.clients {
            let id = self.allocate_client()?;
            clients.push(ClientRecord {
                xid: client.xid,
                descriptor: client.descriptor(id, Generation::new(0))?,
            });
        }
        let active = raw
            .active
            .and_then(|xid| clients.iter().find(|client| client.xid == xid))
            .map(|client| client.descriptor.id);
        Ok(Model {
            current_workspace: raw.current_workspace,
            workspaces: raw.workspaces,
            clients,
            active,
        })
    }

    fn diff_model(&mut self, previous: Model, raw: RawDesktop) -> Result<Model, Failure> {
        let previous_by_xid: HashMap<u32, &ClientRecord> = previous
            .clients
            .iter()
            .map(|client| (client.xid, client))
            .collect();
        let current_xids: HashSet<u32> = raw.clients.iter().map(|client| client.xid).collect();

        for client in &previous.clients {
            if !current_xids.contains(&client.xid) {
                self.push_event(Event::ClientRemoved(client.descriptor.id))?;
            }
        }

        let mut clients = Vec::with_capacity(raw.clients.len());
        for client in raw.clients {
            let record = if let Some(prior) = previous_by_xid.get(&client.xid) {
                let unchanged =
                    client.descriptor(prior.descriptor.id, prior.descriptor.generation)?;
                if unchanged == prior.descriptor {
                    ClientRecord {
                        xid: client.xid,
                        descriptor: unchanged,
                    }
                } else {
                    let generation = prior
                        .descriptor
                        .generation
                        .get()
                        .checked_add(1)
                        .ok_or_else(|| Failure::internal("client generation space is exhausted"))?;
                    let descriptor =
                        client.descriptor(prior.descriptor.id, Generation::new(generation))?;
                    self.push_event(Event::ClientChanged(descriptor.clone()))?;
                    ClientRecord {
                        xid: client.xid,
                        descriptor,
                    }
                }
            } else {
                let descriptor = client.descriptor(self.allocate_client()?, Generation::new(0))?;
                self.push_event(Event::ClientAdded(descriptor.clone()))?;
                ClientRecord {
                    xid: client.xid,
                    descriptor,
                }
            };
            clients.push(record);
        }

        if previous.current_workspace != raw.current_workspace
            || previous.workspaces != raw.workspaces
        {
            self.push_event(Event::WorkspaceChanged(raw.current_workspace))?;
        }
        let active = raw
            .active
            .and_then(|xid| clients.iter().find(|client| client.xid == xid))
            .map(|client| client.descriptor.id);
        if previous.active != active {
            self.push_event(Event::ActiveChanged(active))?;
        }
        Ok(Model {
            current_workspace: raw.current_workspace,
            workspaces: raw.workspaces,
            clients,
            active,
        })
    }

    fn allocate_client(&mut self) -> Result<ClientId, Failure> {
        let id = NonZeroU64::new(self.next_client)
            .ok_or_else(|| Failure::internal("client identity space is exhausted"))?;
        self.next_client = self
            .next_client
            .checked_add(1)
            .ok_or_else(|| Failure::internal("client identity space is exhausted"))?;
        Ok(ClientId::new(id))
    }

    fn push_event(&mut self, event: Event) -> Result<(), Failure> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| Failure::internal("observation sequence space is exhausted"))?;
        if !self.subscribed
            || self.resync_required
            || (!self.subscribed_kinds.is_empty()
                && !self.subscribed_kinds.contains(&event_kind(&event)))
        {
            return Ok(());
        }
        if self.events.len() == MAX_EVENTS {
            self.events.clear();
            self.resync_required = true;
            return Ok(());
        }
        self.events.push_back(EventEnvelope {
            sequence: self.sequence(),
            event,
        });
        Ok(())
    }

    fn current_snapshot(&self) -> Result<DesktopSnapshot, Failure> {
        let model = self
            .model
            .as_ref()
            .ok_or_else(|| Failure::internal("observer has no current model"))?;
        Ok(DesktopSnapshot {
            sequence: self.sequence(),
            current_workspace: model.current_workspace,
            workspaces: BoundedList::new(model.workspaces.clone())
                .map_err(|_| Failure::internal("workspace model exceeded its public bound"))?,
            clients: BoundedList::new(
                model
                    .clients
                    .iter()
                    .map(|client| client.descriptor.clone())
                    .collect(),
            )
            .map_err(|_| Failure::internal("client model exceeded its public bound"))?,
            active: model.active,
        })
    }
}

const fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::ClientAdded(_) => EventKind::ClientAdded,
        Event::ClientChanged(_) => EventKind::ClientChanged,
        Event::ClientRemoved(_) => EventKind::ClientRemoved,
        Event::ActiveChanged(_) => EventKind::ActiveChanged,
        Event::WorkspaceChanged(_) => EventKind::WorkspaceChanged,
        Event::ApplicationsChanged(_) => EventKind::ApplicationsChanged,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Model {
    current_workspace: WorkspaceId,
    workspaces: Vec<WorkspaceDescriptor>,
    clients: Vec<ClientRecord>,
    active: Option<ClientId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClientRecord {
    xid: u32,
    descriptor: ClientDescriptor,
}

struct RawDesktop {
    current_workspace: WorkspaceId,
    workspaces: Vec<WorkspaceDescriptor>,
    clients: Vec<RawClient>,
    active: Option<u32>,
}

struct RawClient {
    xid: u32,
    title: Option<Title>,
    workspace: Option<WorkspaceId>,
    frame: Option<Rect>,
    states: Vec<ClientState>,
    actions: Vec<ClientAction>,
}

impl RawClient {
    fn descriptor(
        &self,
        id: ClientId,
        generation: Generation,
    ) -> Result<ClientDescriptor, Failure> {
        Ok(ClientDescriptor {
            id,
            generation,
            title: self.title.clone(),
            workspace: self.workspace,
            frame: self.frame,
            states: BoundedList::new(self.states.clone())
                .map_err(|_| Failure::internal("client state list exceeded its public bound"))?,
            actions: BoundedList::new(self.actions.clone())
                .map_err(|_| Failure::internal("client action list exceeded its public bound"))?,
        })
    }
}

#[derive(Clone, Copy)]
enum Placement {
    Workspace(WorkspaceId),
    Sticky,
    Unknown,
}

fn scan(
    connection: &RustConnection,
    root: u32,
    atoms: &Atoms,
    scope: ClientScope,
    show_titles: bool,
) -> Result<RawDesktop, Failure> {
    validate_wm(connection, root, atoms)?;
    let workspace_count = required_cardinal(connection, root, atoms.number_of_desktops)?;
    let workspace_count_usize = usize::try_from(workspace_count)
        .map_err(|_| Failure::too_large("workspace count cannot be represented"))?;
    if workspace_count == 0 || workspace_count_usize > MAX_WORKSPACES {
        return Err(Failure::too_large(
            "workspace count is outside the public bound",
        ));
    }
    let current = required_cardinal(connection, root, atoms.current_desktop)?;
    if current >= workspace_count || current > u32::from(u16::MAX) {
        return Err(Failure::malformed(
            "current workspace is outside the advertised range",
        ));
    }
    let current_workspace = WorkspaceId::new(current as u16);
    let workspaces = read_workspaces(connection, root, atoms, workspace_count)?;
    let supported = property32(
        connection,
        root,
        atoms.supported,
        AtomEnum::ATOM.into(),
        MAX_ROOT_ATOMS,
    )?
    .unwrap_or_default();

    let clients = match scope {
        ClientScope::None => Vec::new(),
        ClientScope::CurrentWorkspace | ClientScope::AllWorkspaces => {
            let xids = client_list(connection, root, atoms)?;
            let mut clients = Vec::with_capacity(xids.len());
            for xid in xids {
                if let Some(client) = read_client(
                    connection,
                    root,
                    atoms,
                    &supported,
                    xid,
                    current_workspace,
                    workspace_count,
                    scope,
                    show_titles,
                ) {
                    clients.push(client);
                }
            }
            clients
        }
    };
    let visible_xids: HashSet<u32> = clients.iter().map(|client| client.xid).collect();
    let active = optional_window(connection, root, atoms.active_window)
        .filter(|xid| visible_xids.contains(xid));
    Ok(RawDesktop {
        current_workspace,
        workspaces,
        clients,
        active,
    })
}

fn validate_wm(connection: &RustConnection, root: u32, atoms: &Atoms) -> Result<(), Failure> {
    let Some(check) = optional_window(connection, root, atoms.supporting_wm_check) else {
        return Err(Failure::unsupported(
            "no conforming EWMH window manager is active",
        ));
    };
    if optional_window(connection, check, atoms.supporting_wm_check) != Some(check) {
        return Err(Failure::unsupported(
            "the EWMH window-manager check is stale",
        ));
    }
    Ok(())
}

fn read_workspaces(
    connection: &RustConnection,
    root: u32,
    atoms: &Atoms,
    count: u32,
) -> Result<Vec<WorkspaceDescriptor>, Failure> {
    let count = usize::try_from(count)
        .map_err(|_| Failure::too_large("workspace count cannot be represented"))?;
    let names = property_bytes(
        connection,
        root,
        atoms.desktop_names,
        atoms.utf8,
        MAX_WORKSPACES * (MAX_WORKSPACE_NAME_BYTES + 1),
    )
    .ok()
    .flatten()
    .map(|bytes| parse_workspace_names(&bytes, count))
    .unwrap_or_else(|| vec![None; count]);
    let work_areas = property32(
        connection,
        root,
        atoms.workarea,
        AtomEnum::CARDINAL.into(),
        MAX_WORKSPACES * 4,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let mut workspaces = Vec::with_capacity(count);
    for index in 0..count {
        let work_area = work_areas
            .get(index * 4..index * 4 + 4)
            .and_then(rect_from_cardinals);
        workspaces.push(WorkspaceDescriptor {
            id: WorkspaceId::new(
                u16::try_from(index)
                    .map_err(|_| Failure::internal("workspace index exceeded its bound"))?,
            ),
            name: names.get(index).cloned().flatten(),
            work_area,
        });
    }
    Ok(workspaces)
}

fn parse_workspace_names(bytes: &[u8], count: usize) -> Vec<Option<WorkspaceName>> {
    let mut names = bytes
        .split(|byte| *byte == 0)
        .take(count)
        .map(|name| {
            std::str::from_utf8(name)
                .ok()
                .filter(|name| !name.is_empty())
                .and_then(|name| WorkspaceName::new(name).ok())
        })
        .collect::<Vec<_>>();
    names.resize(count, None);
    names
}

fn client_list(connection: &RustConnection, root: u32, atoms: &Atoms) -> Result<Vec<u32>, Failure> {
    let stacking = property32(
        connection,
        root,
        atoms.client_list_stacking,
        AtomEnum::WINDOW.into(),
        MAX_CLIENTS,
    )?;
    let clients = match stacking {
        Some(clients) => clients,
        None => property32(
            connection,
            root,
            atoms.client_list,
            AtomEnum::WINDOW.into(),
            MAX_CLIENTS,
        )?
        .unwrap_or_default(),
    };
    let mut seen = HashSet::with_capacity(clients.len());
    Ok(clients
        .into_iter()
        .filter(|xid| *xid != NONE && seen.insert(*xid))
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn read_client(
    connection: &RustConnection,
    root: u32,
    atoms: &Atoms,
    supported: &[u32],
    xid: u32,
    current_workspace: WorkspaceId,
    workspace_count: u32,
    scope: ClientScope,
    show_titles: bool,
) -> Option<RawClient> {
    let states = client_states(connection, xid, atoms);
    let placement = client_placement(connection, xid, atoms, &states, workspace_count);
    let in_scope = match scope {
        ClientScope::None => false,
        ClientScope::AllWorkspaces => true,
        ClientScope::CurrentWorkspace => match placement {
            Placement::Sticky => true,
            Placement::Workspace(workspace) => workspace == current_workspace,
            Placement::Unknown => false,
        },
    };
    if !in_scope {
        return None;
    }
    let workspace = match placement {
        Placement::Workspace(workspace) => Some(workspace),
        Placement::Sticky | Placement::Unknown => None,
    };
    Some(RawClient {
        xid,
        title: show_titles
            .then(|| client_title(connection, xid, atoms))
            .flatten(),
        workspace,
        frame: client_frame(connection, root, xid, atoms),
        actions: client_actions(connection, xid, atoms, supported),
        states,
    })
}

fn client_placement(
    connection: &RustConnection,
    xid: u32,
    atoms: &Atoms,
    states: &[ClientState],
    workspace_count: u32,
) -> Placement {
    if states.contains(&ClientState::Sticky) {
        return Placement::Sticky;
    }
    match optional_cardinal(connection, xid, atoms.wm_desktop) {
        Some(u32::MAX) => Placement::Sticky,
        Some(workspace) if workspace < workspace_count && workspace <= u32::from(u16::MAX) => {
            Placement::Workspace(WorkspaceId::new(workspace as u16))
        }
        _ => Placement::Unknown,
    }
}

fn client_title(connection: &RustConnection, xid: u32, atoms: &Atoms) -> Option<Title> {
    for property in [atoms.wm_visible_name, atoms.wm_name] {
        if let Some(bytes) = property_bytes(connection, xid, property, atoms.utf8, MAX_TITLE_BYTES)
            .ok()
            .flatten()
        {
            if let Some(title) = std::str::from_utf8(&bytes)
                .ok()
                .filter(|title| !title.is_empty())
                .and_then(|title| Title::new(title).ok())
            {
                return Some(title);
            }
        }
    }
    None
}

fn client_startup_id_matches(
    connection: &RustConnection,
    xid: u32,
    atoms: &Atoms,
    expected: &[u8],
) -> bool {
    let leader = optional_window(connection, xid, atoms.client_leader);
    [Some(xid), leader].into_iter().flatten().any(|window| {
        property_bytes(
            connection,
            window,
            atoms.startup_id,
            atoms.utf8,
            MAX_STARTUP_ID_BYTES,
        )
        .ok()
        .flatten()
        .is_some_and(|value| value == expected)
    })
}

fn client_states(connection: &RustConnection, xid: u32, atoms: &Atoms) -> Vec<ClientState> {
    let values = property32(
        connection,
        xid,
        atoms.wm_state,
        AtomEnum::ATOM.into(),
        MAX_CLIENT_ATOMS,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    [
        (atoms.state_above, ClientState::Above),
        (atoms.state_below, ClientState::Below),
        (atoms.state_fullscreen, ClientState::Fullscreen),
        (atoms.state_hidden, ClientState::Hidden),
        (atoms.state_maximized_horz, ClientState::MaximizedHorizontal),
        (atoms.state_maximized_vert, ClientState::MaximizedVertical),
        (atoms.state_demands_attention, ClientState::DemandsAttention),
        (atoms.state_sticky, ClientState::Sticky),
        (atoms.state_shaded, ClientState::Shaded),
    ]
    .into_iter()
    .filter_map(|(atom, state)| values.contains(&atom).then_some(state))
    .collect()
}

fn client_actions(
    connection: &RustConnection,
    xid: u32,
    atoms: &Atoms,
    supported: &[u32],
) -> Vec<ClientAction> {
    let allowed = property32(
        connection,
        xid,
        atoms.wm_allowed_actions,
        AtomEnum::ATOM.into(),
        MAX_CLIENT_ATOMS,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let protocols = property32(
        connection,
        xid,
        atoms.wm_protocols,
        AtomEnum::ATOM.into(),
        MAX_CLIENT_ATOMS,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let mut actions = Vec::with_capacity(5);
    if supported.contains(&atoms.active_window) {
        actions.push(ClientAction::Activate);
    }
    if supported.contains(&atoms.close_window)
        && allowed.contains(&atoms.action_close)
        && protocols.contains(&atoms.wm_delete_window)
    {
        actions.push(ClientAction::Close);
    }
    if supported.contains(&atoms.wm_desktop) && allowed.contains(&atoms.action_change_desktop) {
        actions.push(ClientAction::ChangeWorkspace);
    }
    if supported.contains(&atoms.wm_state)
        && (supported.contains(&atoms.state_demands_attention)
            || [
                (atoms.action_above, atoms.state_above),
                (atoms.action_below, atoms.state_below),
                (atoms.action_fullscreen, atoms.state_fullscreen),
                (atoms.action_maximize_horz, atoms.state_maximized_horz),
                (atoms.action_maximize_vert, atoms.state_maximized_vert),
                (atoms.action_shade, atoms.state_shaded),
                (atoms.action_stick, atoms.state_sticky),
            ]
            .iter()
            .any(|(action, state)| allowed.contains(action) && supported.contains(state)))
    {
        actions.push(ClientAction::ChangeState);
    }
    if supported.contains(&atoms.moveresize_window)
        && (allowed.contains(&atoms.action_move) || allowed.contains(&atoms.action_resize))
    {
        actions.push(ClientAction::ChangeGeometry);
    }
    actions
}

fn client_frame(connection: &RustConnection, root: u32, xid: u32, atoms: &Atoms) -> Option<Rect> {
    let geometry = connection.get_geometry(xid).ok()?.reply().ok()?;
    let translated = connection
        .translate_coordinates(xid, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    let extents = client_frame_extents(connection, xid, atoms);
    let left = i32::try_from(extents[0]).ok()?;
    let right = extents[1];
    let top = i32::try_from(extents[2]).ok()?;
    let bottom = extents[3];
    Some(Rect {
        x: i32::from(translated.dst_x).checked_sub(left)?,
        y: i32::from(translated.dst_y).checked_sub(top)?,
        width: u32::from(geometry.width)
            .checked_add(extents[0])?
            .checked_add(right)?,
        height: u32::from(geometry.height)
            .checked_add(extents[2])?
            .checked_add(bottom)?,
    })
}

fn client_frame_extents(connection: &RustConnection, xid: u32, atoms: &Atoms) -> [u32; 4] {
    property32(
        connection,
        xid,
        atoms.frame_extents,
        AtomEnum::CARDINAL.into(),
        4,
    )
    .ok()
    .flatten()
    .and_then(|values| <[u32; 4]>::try_from(values).ok())
    .unwrap_or([0; 4])
}

fn rect_from_cardinals(values: &[u32]) -> Option<Rect> {
    Some(Rect {
        x: i32::try_from(*values.first()?).ok()?,
        y: i32::try_from(*values.get(1)?).ok()?,
        width: *values.get(2)?,
        height: *values.get(3)?,
    })
    .filter(|rect| rect.width != 0 && rect.height != 0)
}

fn required_cardinal(
    connection: &RustConnection,
    window: u32,
    property: u32,
) -> Result<u32, Failure> {
    optional_cardinal(connection, window, property)
        .ok_or_else(|| Failure::unsupported("required EWMH workspace property is unavailable"))
}

fn optional_cardinal(connection: &RustConnection, window: u32, property: u32) -> Option<u32> {
    property32(connection, window, property, AtomEnum::CARDINAL.into(), 1)
        .ok()
        .flatten()
        .filter(|values| values.len() == 1)
        .map(|values| values[0])
}

fn optional_window(connection: &RustConnection, window: u32, property: u32) -> Option<u32> {
    property32(connection, window, property, AtomEnum::WINDOW.into(), 1)
        .ok()
        .flatten()
        .filter(|values| values.len() == 1 && values[0] != NONE)
        .map(|values| values[0])
}

fn property32(
    connection: &RustConnection,
    window: u32,
    property: u32,
    expected_type: u32,
    max_items: usize,
) -> Result<Option<Vec<u32>>, Failure> {
    let long_length = u32::try_from(max_items)
        .map_err(|_| Failure::internal("property item bound cannot be represented"))?;
    let reply = connection
        .get_property(false, window, property, expected_type, 0, long_length)
        .map_err(|_| Failure::unavailable("cannot request an X11 property"))?
        .reply()
        .map_err(|_| Failure::unavailable("cannot read an X11 property"))?;
    if reply.type_ == NONE {
        return Ok(None);
    }
    if reply.type_ != expected_type || reply.format != 32 {
        return Err(Failure::malformed(
            "an EWMH property has the wrong type or format",
        ));
    }
    if reply.bytes_after != 0 {
        return Err(Failure::too_large(
            "an EWMH property exceeds its public bound",
        ));
    }
    let values = reply
        .value32()
        .ok_or_else(|| Failure::malformed("an EWMH property is not 32-bit"))?
        .collect::<Vec<_>>();
    if values.len() > max_items {
        return Err(Failure::too_large(
            "an EWMH property exceeds its public bound",
        ));
    }
    Ok(Some(values))
}

fn property_bytes(
    connection: &RustConnection,
    window: u32,
    property: u32,
    expected_type: u32,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, Failure> {
    let longs = max_bytes
        .checked_add(3)
        .and_then(|bytes| bytes.checked_div(4))
        .and_then(|longs| u32::try_from(longs).ok())
        .ok_or_else(|| Failure::internal("property byte bound cannot be represented"))?;
    let reply = connection
        .get_property(false, window, property, expected_type, 0, longs)
        .map_err(|_| Failure::unavailable("cannot request an X11 text property"))?
        .reply()
        .map_err(|_| Failure::unavailable("cannot read an X11 text property"))?;
    if reply.type_ == NONE {
        return Ok(None);
    }
    if reply.type_ != expected_type || reply.format != 8 {
        return Err(Failure::malformed(
            "an EWMH text property has the wrong type or format",
        ));
    }
    if reply.bytes_after != 0 || reply.value.len() > max_bytes {
        return Err(Failure::too_large(
            "an EWMH text property exceeds its public bound",
        ));
    }
    Ok(Some(reply.value))
}

struct Atoms {
    supported: u32,
    client_list: u32,
    client_list_stacking: u32,
    number_of_desktops: u32,
    current_desktop: u32,
    desktop_names: u32,
    active_window: u32,
    workarea: u32,
    supporting_wm_check: u32,
    wm_visible_name: u32,
    wm_name: u32,
    wm_desktop: u32,
    wm_state: u32,
    wm_allowed_actions: u32,
    frame_extents: u32,
    moveresize_window: u32,
    close_window: u32,
    wm_protocols: u32,
    wm_delete_window: u32,
    client_leader: u32,
    startup_id: u32,
    utf8: u32,
    state_above: u32,
    state_below: u32,
    state_fullscreen: u32,
    state_hidden: u32,
    state_maximized_horz: u32,
    state_maximized_vert: u32,
    state_demands_attention: u32,
    state_sticky: u32,
    state_shaded: u32,
    action_above: u32,
    action_below: u32,
    action_fullscreen: u32,
    action_maximize_horz: u32,
    action_maximize_vert: u32,
    action_shade: u32,
    action_stick: u32,
    action_close: u32,
    action_change_desktop: u32,
    action_move: u32,
    action_resize: u32,
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, Failure> {
        let atom = |name: &[u8]| {
            connection
                .intern_atom(false, name)
                .map_err(|_| Failure::unavailable("cannot request an X11 atom"))?
                .reply()
                .map(|reply| reply.atom)
                .map_err(|_| Failure::unavailable("cannot intern an X11 atom"))
        };
        Ok(Self {
            supported: atom(b"_NET_SUPPORTED")?,
            client_list: atom(b"_NET_CLIENT_LIST")?,
            client_list_stacking: atom(b"_NET_CLIENT_LIST_STACKING")?,
            number_of_desktops: atom(b"_NET_NUMBER_OF_DESKTOPS")?,
            current_desktop: atom(b"_NET_CURRENT_DESKTOP")?,
            desktop_names: atom(b"_NET_DESKTOP_NAMES")?,
            active_window: atom(b"_NET_ACTIVE_WINDOW")?,
            workarea: atom(b"_NET_WORKAREA")?,
            supporting_wm_check: atom(b"_NET_SUPPORTING_WM_CHECK")?,
            wm_visible_name: atom(b"_NET_WM_VISIBLE_NAME")?,
            wm_name: atom(b"_NET_WM_NAME")?,
            wm_desktop: atom(b"_NET_WM_DESKTOP")?,
            wm_state: atom(b"_NET_WM_STATE")?,
            wm_allowed_actions: atom(b"_NET_WM_ALLOWED_ACTIONS")?,
            frame_extents: atom(b"_NET_FRAME_EXTENTS")?,
            moveresize_window: atom(b"_NET_MOVERESIZE_WINDOW")?,
            close_window: atom(b"_NET_CLOSE_WINDOW")?,
            wm_protocols: atom(b"WM_PROTOCOLS")?,
            wm_delete_window: atom(b"WM_DELETE_WINDOW")?,
            client_leader: atom(b"WM_CLIENT_LEADER")?,
            startup_id: atom(b"_NET_STARTUP_ID")?,
            utf8: atom(b"UTF8_STRING")?,
            state_above: atom(b"_NET_WM_STATE_ABOVE")?,
            state_below: atom(b"_NET_WM_STATE_BELOW")?,
            state_fullscreen: atom(b"_NET_WM_STATE_FULLSCREEN")?,
            state_hidden: atom(b"_NET_WM_STATE_HIDDEN")?,
            state_maximized_horz: atom(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
            state_maximized_vert: atom(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
            state_demands_attention: atom(b"_NET_WM_STATE_DEMANDS_ATTENTION")?,
            state_sticky: atom(b"_NET_WM_STATE_STICKY")?,
            state_shaded: atom(b"_NET_WM_STATE_SHADED")?,
            action_above: atom(b"_NET_WM_ACTION_ABOVE")?,
            action_below: atom(b"_NET_WM_ACTION_BELOW")?,
            action_fullscreen: atom(b"_NET_WM_ACTION_FULLSCREEN")?,
            action_maximize_horz: atom(b"_NET_WM_ACTION_MAXIMIZE_HORZ")?,
            action_maximize_vert: atom(b"_NET_WM_ACTION_MAXIMIZE_VERT")?,
            action_shade: atom(b"_NET_WM_ACTION_SHADE")?,
            action_stick: atom(b"_NET_WM_ACTION_STICK")?,
            action_close: atom(b"_NET_WM_ACTION_CLOSE")?,
            action_change_desktop: atom(b"_NET_WM_ACTION_CHANGE_DESKTOP")?,
            action_move: atom(b"_NET_WM_ACTION_MOVE")?,
            action_resize: atom(b"_NET_WM_ACTION_RESIZE")?,
        })
    }
}

pub(crate) struct Failure {
    pub(crate) code: ErrorCode,
    pub(crate) retry: Retry,
    pub(crate) message: &'static str,
    pub(crate) current_generation: Option<Generation>,
    pub(crate) current_sequence: Option<Sequence>,
}

impl Failure {
    pub(super) const fn unavailable(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Unavailable,
            retry: Retry::Reconnect,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    const fn unsupported(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Unsupported,
            retry: Retry::Never,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    pub(super) const fn invalid(message: &'static str) -> Self {
        Self {
            code: ErrorCode::InvalidArgument,
            retry: Retry::Never,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    const fn malformed(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Malformed,
            retry: Retry::Reobserve,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    const fn too_large(message: &'static str) -> Self {
        Self {
            code: ErrorCode::TooLarge,
            retry: Retry::Never,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    pub(crate) const fn internal(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Internal,
            retry: Retry::Never,
            message,
            current_generation: None,
            current_sequence: None,
        }
    }

    const fn resync(sequence: u64) -> Self {
        Self {
            code: ErrorCode::ResyncRequired,
            retry: Retry::Reobserve,
            message: "event cursor is outside the retained observation history",
            current_generation: None,
            current_sequence: Some(Sequence::new(sequence)),
        }
    }
}
