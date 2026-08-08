//! Strict revision-3 messages and core Tier 0 values.

use serde::{Deserialize, Serialize};

use crate::{
    BoundedList, BoundedText, ClientId, Generation, LaunchToken, MAX_REQUEST_FRAME_BYTES,
    MAX_RESPONSE_FRAME_BYTES, PROTOCOL_NAME, PROTOCOL_REVISION, RequestId, Sequence, SessionId,
    Validate, WorkspaceId,
};

/// Maximum peer/component name length in bytes.
pub const MAX_NAME_BYTES: usize = 128;
/// Maximum version string length in bytes.
pub const MAX_VERSION_BYTES: usize = 64;
/// Maximum declared purpose length in bytes.
pub const MAX_PURPOSE_BYTES: usize = 256;
/// Maximum diagnostic length in bytes.
pub const MAX_DIAGNOSTIC_BYTES: usize = 512;
/// Maximum title length in bytes.
pub const MAX_TITLE_BYTES: usize = 1024;
/// Maximum desktop application ID length in bytes.
pub const MAX_APPLICATION_ID_BYTES: usize = 256;
/// Maximum workspace name length in bytes.
pub const MAX_WORKSPACE_NAME_BYTES: usize = 256;
/// Maximum capabilities in an opening message.
pub const MAX_CAPABILITIES: usize = 32;
/// Maximum backend features in an opening message.
pub const MAX_FEATURES: usize = 16;
/// Maximum workspaces in one snapshot.
pub const MAX_WORKSPACES: usize = 128;
/// Maximum clients in one snapshot.
pub const MAX_CLIENTS: usize = 1024;
/// Maximum events in one poll response.
pub const MAX_EVENTS: usize = 1024;
/// Maximum applications in one page.
pub const MAX_APPLICATIONS: usize = 256;
/// Longest event poll wait.
pub const MAX_POLL_WAIT_MS: u32 = 30_000;

/// A component or display name.
pub type Name = BoundedText<MAX_NAME_BYTES>;
/// A component version.
pub type Version = BoundedText<MAX_VERSION_BYTES>;
/// A peer purpose shown to the session owner.
pub type Purpose = BoundedText<MAX_PURPOSE_BYTES>;
/// A bounded human-readable diagnostic.
pub type Diagnostic = BoundedText<MAX_DIAGNOSTIC_BYTES>;
/// A client title, when granted.
pub type Title = BoundedText<MAX_TITLE_BYTES>;
/// A canonical desktop application identifier.
pub type ApplicationId = BoundedText<MAX_APPLICATION_ID_BYTES>;
/// A workspace name.
pub type WorkspaceName = BoundedText<MAX_WORKSPACE_NAME_BYTES>;

/// One complete peer-to-provider message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ClientMessage {
    /// Required opening message.
    Hello(Hello),
    /// One provider call after a successful opening.
    Request(Request),
    /// Voluntary peer shutdown.
    Goodbye(Goodbye),
}

impl Validate for ClientMessage {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Hello(value) => value.validate(),
            Self::Request(value) => value.validate(),
            Self::Goodbye(value) => value.validate(),
        }
    }
}

/// One complete provider-to-peer message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ServerMessage {
    /// Successful opening response.
    Welcome(Welcome),
    /// Response paired with one request.
    Response(Response),
    /// Provider-initiated terminal message.
    Goodbye(Goodbye),
}

impl Validate for ServerMessage {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Welcome(value) => value.validate(),
            Self::Response(value) => value.validate(),
            Self::Goodbye(value) => value.validate(),
        }
    }
}

/// Peer implementation metadata. It is descriptive, never an authorization
/// identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeerInfo {
    /// Stable implementation name.
    pub name: Name,
    /// Implementation version.
    pub version: Version,
    /// Bounded reason for requesting a seat.
    pub purpose: Purpose,
}

impl Validate for PeerInfo {
    fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() {
            return Err("peer name is empty");
        }
        if self.version.is_empty() {
            return Err("peer version is empty");
        }
        if self.purpose.is_empty() {
            return Err("peer purpose is empty");
        }
        Ok(())
    }
}

/// Opening request. Protocol/revision mismatch remains decodable so a provider
/// can return a precise incompatibility response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// Protocol name requested by the peer.
    pub protocol: Name,
    /// Exact wire revision requested by the peer.
    pub revision: u16,
    /// Descriptive peer information.
    pub peer: PeerInfo,
    /// Capabilities requested in canonical order.
    pub requested: BoundedList<Capability, MAX_CAPABILITIES>,
}

impl Hello {
    /// Returns whether the opening names this exact wire contract.
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        self.protocol.as_str() == PROTOCOL_NAME && self.revision == PROTOCOL_REVISION
    }
}

impl Validate for Hello {
    fn validate(&self) -> Result<(), &'static str> {
        self.peer.validate()?;
        strictly_ordered(&self.requested)
            .then_some(())
            .ok_or("requested capabilities must be unique and canonically ordered")
    }
}

/// Provider implementation metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInfo {
    /// Stable provider name.
    pub name: Name,
    /// Provider version.
    pub version: Version,
}

impl Validate for ProviderInfo {
    fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() || self.version.is_empty() {
            return Err("provider name and version must be nonempty");
        }
        Ok(())
    }
}

/// Backend family serving the session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Standalone EWMH X11 provider.
    X11Ewmh,
}

/// Assurance level attached to every provider result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Assurance {
    /// Provider-observed Tier 0 behavior beside a foreign window manager.
    Tier0,
}

/// A capability atom granted to one authenticated session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Observe desktop structure.
    ObserveStructure,
    /// Observe client titles.
    ObserveTitles,
    /// Subscribe to bounded changes.
    ObserveEvents,
    /// Request client activation.
    ManageActivate,
    /// Request polite close.
    ManageClose,
    /// Request workspace changes.
    ManageWorkspace,
    /// Request supported client state changes.
    ManageState,
    /// Request supported geometry changes.
    ManageGeometry,
    /// List applications available under current policy.
    LaunchList,
    /// Launch a policy-approved desktop entry.
    LaunchExecute,
}

/// Functionality implemented by the current provider/backend.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// Bounded EWMH observation.
    EwmhObservation,
    /// EWMH management with post-request observation.
    EwmhManagement,
    /// Controlled XDG desktop-entry launch.
    DesktopLaunch,
    /// Optional visible-client capture.
    ClientVisibleCapture,
    /// Optional obscured-client capture.
    ObscuredCapture,
    /// Optional output capture.
    OutputCapture,
    /// Optional X11 input injection.
    InputInjection,
    /// Optional human-activity observation.
    HumanActivity,
    /// Optional accessibility projection.
    Accessibility,
}

/// Provider limits relevant to client allocation and scheduling.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Maximum peer-to-provider JSON payload bytes.
    pub request_frame_bytes: u32,
    /// Maximum provider-to-peer JSON payload bytes.
    pub response_frame_bytes: u32,
    /// Maximum events returned in one poll.
    pub events_per_poll: u16,
    /// Maximum poll wait.
    pub poll_wait_ms: u32,
}

impl Validate for Limits {
    fn validate(&self) -> Result<(), &'static str> {
        if self.request_frame_bytes == 0
            || self.request_frame_bytes as usize > MAX_REQUEST_FRAME_BYTES
            || self.response_frame_bytes == 0
            || self.response_frame_bytes as usize > MAX_RESPONSE_FRAME_BYTES
            || self.events_per_poll == 0
            || usize::from(self.events_per_poll) > MAX_EVENTS
            || self.poll_wait_ms > MAX_POLL_WAIT_MS
        {
            return Err("provider limits are outside revision bounds");
        }
        Ok(())
    }
}

/// Successful authenticated opening response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    /// Exact protocol name.
    pub protocol: Name,
    /// Exact wire revision.
    pub revision: u16,
    /// Provider-selected session identity.
    pub session: SessionId,
    /// Provider implementation metadata.
    pub provider: ProviderInfo,
    /// Backend family.
    pub backend: Backend,
    /// Honest assurance level.
    pub assurance: Assurance,
    /// Implemented features in canonical order.
    pub features: BoundedList<Feature, MAX_FEATURES>,
    /// Granted capability atoms in canonical order.
    pub granted: BoundedList<Capability, MAX_CAPABILITIES>,
    /// Published resource bounds.
    pub limits: Limits,
}

impl Validate for Welcome {
    fn validate(&self) -> Result<(), &'static str> {
        if self.protocol.as_str() != PROTOCOL_NAME || self.revision != PROTOCOL_REVISION {
            return Err("welcome protocol or revision is incompatible");
        }
        self.provider.validate()?;
        self.limits.validate()?;
        if !strictly_ordered(&self.features) || !strictly_ordered(&self.granted) {
            return Err("welcome features and grants must be canonically ordered");
        }
        Ok(())
    }
}

/// Terminal session reason.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Goodbye {
    /// Stable machine-readable reason.
    pub code: ErrorCode,
    /// Optional bounded diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Diagnostic>,
}

impl Validate for Goodbye {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .message
            .as_ref()
            .is_some_and(|message| message.is_empty())
        {
            return Err("goodbye diagnostic is empty");
        }
        Ok(())
    }
}

/// One identified provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Peer-selected request identity.
    pub id: RequestId,
    /// Typed operation.
    pub call: Call,
}

impl Validate for Request {
    fn validate(&self) -> Result<(), &'static str> {
        self.call.validate()
    }
}

/// Empty arguments that reject every unknown field.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// Typed Tier 0 operations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "name", content = "arguments", deny_unknown_fields)]
pub enum Call {
    /// Report the live authenticated seat.
    #[serde(rename = "seat.status")]
    SeatStatus(Empty),
    /// Take a bounded current snapshot.
    #[serde(rename = "desktop.snapshot")]
    DesktopSnapshot(Empty),
    /// Begin a bounded event stream.
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(SubscribeRequest),
    /// Poll a bounded event stream.
    #[serde(rename = "events.poll")]
    EventsPoll(PollRequest),
    /// Request client activation.
    #[serde(rename = "client.activate")]
    ClientActivate(TargetRequest),
    /// Request polite client close.
    #[serde(rename = "client.close")]
    ClientClose(TargetRequest),
    /// Request current workspace change.
    #[serde(rename = "workspace.switch")]
    WorkspaceSwitch(WorkspaceRequest),
    /// Request moving a client to a workspace.
    #[serde(rename = "client.workspace")]
    ClientWorkspace(ClientWorkspaceRequest),
    /// Request an advertised state change.
    #[serde(rename = "client.state")]
    ClientState(ClientStateRequest),
    /// Request advertised geometry change.
    #[serde(rename = "client.geometry")]
    ClientGeometry(ClientGeometryRequest),
    /// List policy-visible desktop applications.
    #[serde(rename = "applications.list")]
    ApplicationsList(ApplicationListRequest),
    /// Launch one policy-approved desktop entry.
    #[serde(rename = "application.launch")]
    ApplicationLaunch(ApplicationLaunchRequest),
}

impl Call {
    /// Returns the capability atom required by this call.
    #[must_use]
    pub const fn required_capability(&self) -> Capability {
        match self {
            Self::SeatStatus(_) | Self::DesktopSnapshot(_) => Capability::ObserveStructure,
            Self::EventsSubscribe(_) | Self::EventsPoll(_) => Capability::ObserveEvents,
            Self::ClientActivate(_) => Capability::ManageActivate,
            Self::ClientClose(_) => Capability::ManageClose,
            Self::WorkspaceSwitch(_) | Self::ClientWorkspace(_) => Capability::ManageWorkspace,
            Self::ClientState(_) => Capability::ManageState,
            Self::ClientGeometry(_) => Capability::ManageGeometry,
            Self::ApplicationsList(_) => Capability::LaunchList,
            Self::ApplicationLaunch(_) => Capability::LaunchExecute,
        }
    }
}

impl Validate for Call {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::SeatStatus(_) | Self::DesktopSnapshot(_) => Ok(()),
            Self::EventsSubscribe(value) => value.validate(),
            Self::EventsPoll(value) => value.validate(),
            Self::ClientActivate(value) | Self::ClientClose(value) => value.validate(),
            Self::WorkspaceSwitch(value) => value.validate(),
            Self::ClientWorkspace(value) => value.validate(),
            Self::ClientState(value) => value.validate(),
            Self::ClientGeometry(value) => value.validate(),
            Self::ApplicationsList(value) => value.validate(),
            Self::ApplicationLaunch(value) => value.validate(),
        }
    }
}

/// Provider-local target freshness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    /// Opaque session client handle.
    pub client: ClientId,
    /// Last descriptor generation observed by the peer.
    pub generation: Generation,
}

impl Validate for TargetRequest {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// Event classes a peer may request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Client appeared.
    ClientAdded,
    /// Visible client descriptor changed.
    ClientChanged,
    /// Client disappeared.
    ClientRemoved,
    /// Active client changed.
    ActiveChanged,
    /// Workspace metadata/current selection changed.
    WorkspaceChanged,
    /// Application catalog changed.
    ApplicationsChanged,
}

/// Event subscription arguments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeRequest {
    /// Requested classes in canonical order; empty means all classes.
    #[serde(default)]
    pub kinds: BoundedList<EventKind, 8>,
}

impl Validate for SubscribeRequest {
    fn validate(&self) -> Result<(), &'static str> {
        strictly_ordered(&self.kinds)
            .then_some(())
            .ok_or("event kinds must be unique and canonically ordered")
    }
}

/// Event poll arguments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PollRequest {
    /// Cursor received from the initial snapshot or prior poll.
    pub after: Sequence,
    /// Requested maximum events.
    pub limit: u16,
    /// Requested wait before an empty response.
    pub wait_ms: u32,
}

impl Validate for PollRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if self.limit == 0
            || usize::from(self.limit) > MAX_EVENTS
            || self.wait_ms > MAX_POLL_WAIT_MS
        {
            return Err("event poll limit or wait is outside revision bounds");
        }
        Ok(())
    }
}

/// Workspace-switch arguments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRequest {
    /// Desired EWMH workspace index.
    pub workspace: WorkspaceId,
    /// Snapshot sequence on which the decision was based.
    pub sequence: Sequence,
}

impl Validate for WorkspaceRequest {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// Client-to-workspace arguments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientWorkspaceRequest {
    /// Fresh target.
    #[serde(flatten)]
    pub target: TargetRequest,
    /// Desired workspace index.
    pub workspace: WorkspaceId,
}

impl Validate for ClientWorkspaceRequest {
    fn validate(&self) -> Result<(), &'static str> {
        self.target.validate()
    }
}

/// Supported EWMH state atom expressed without an X11 atom identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientState {
    /// Above ordinary windows.
    Above,
    /// Below ordinary windows.
    Below,
    /// Fullscreen.
    Fullscreen,
    /// Hidden/minimized.
    Hidden,
    /// Horizontally maximized.
    MaximizedHorizontal,
    /// Vertically maximized.
    MaximizedVertical,
    /// Demands attention.
    DemandsAttention,
    /// Present on every workspace.
    Sticky,
    /// Titlebar-only shading.
    Shaded,
}

/// Requested EWMH state transition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateAction {
    /// Ensure the state is present.
    Add,
    /// Ensure the state is absent.
    Remove,
    /// Toggle the currently observed state.
    Toggle,
}

/// Client state-change arguments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientStateRequest {
    /// Fresh target.
    #[serde(flatten)]
    pub target: TargetRequest,
    /// State to change.
    pub state: ClientState,
    /// Desired operation.
    pub action: StateAction,
}

impl Validate for ClientStateRequest {
    fn validate(&self) -> Result<(), &'static str> {
        self.target.validate()
    }
}

/// Signed position and nonempty extent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    /// Left coordinate.
    pub x: i32,
    /// Top coordinate.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Validate for Rect {
    fn validate(&self) -> Result<(), &'static str> {
        if self.width == 0 || self.height == 0 {
            return Err("rectangle extent must be nonzero");
        }
        Ok(())
    }
}

/// Client geometry-change arguments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientGeometryRequest {
    /// Fresh target.
    #[serde(flatten)]
    pub target: TargetRequest,
    /// Requested public frame rectangle.
    pub frame: Rect,
}

impl Validate for ClientGeometryRequest {
    fn validate(&self) -> Result<(), &'static str> {
        self.target.validate()?;
        self.frame.validate()
    }
}

/// Application catalog page request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationListRequest {
    /// Opaque index returned by the prior page; zero begins a scan.
    #[serde(default)]
    pub cursor: u32,
    /// Requested maximum entries.
    pub limit: u16,
}

impl Validate for ApplicationListRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if self.limit == 0 || usize::from(self.limit) > MAX_APPLICATIONS {
            return Err("application page limit is outside revision bounds");
        }
        Ok(())
    }
}

/// Controlled desktop-entry launch arguments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationLaunchRequest {
    /// Canonical desktop ID from the current catalog.
    pub application: ApplicationId,
}

impl Validate for ApplicationLaunchRequest {
    fn validate(&self) -> Result<(), &'static str> {
        if self.application.is_empty()
            || !self.application.ends_with(".desktop")
            || self.application.contains(['/', '\0'])
        {
            return Err("application ID is not canonical");
        }
        Ok(())
    }
}

/// One response paired to a request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// Original request identity.
    pub id: RequestId,
    /// Success or typed failure.
    pub outcome: Outcome,
}

impl Validate for Response {
    fn validate(&self) -> Result<(), &'static str> {
        self.outcome.validate()
    }
}

/// Mutually exclusive request result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "status",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Outcome {
    /// Successful typed result.
    Ok(Reply),
    /// Typed refusal/failure.
    Error(ProtocolError),
}

impl Validate for Outcome {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Ok(reply) => reply.validate(),
            Self::Error(error) => error.validate(),
        }
    }
}

/// Stable error vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// No provider/source is available.
    Unavailable,
    /// Peer and provider revisions differ.
    IncompatibleRevision,
    /// Grant or global policy refused without sending an operation.
    Refused,
    /// Missing, hidden, or out-of-scope client.
    NoSuchClient,
    /// Provider-local freshness changed before send.
    Stale,
    /// Backend or target does not advertise the operation.
    Unsupported,
    /// A valid request was sent but not observed before deadline.
    TimedOut,
    /// Arguments violate the typed contract.
    InvalidArgument,
    /// Framing or strict schema is malformed.
    Malformed,
    /// A published byte/item bound was exceeded.
    TooLarge,
    /// Provider failed internally.
    Internal,
    /// Event backlog was replaced by a required resynchronization.
    ResyncRequired,
    /// Grant was removed or narrowed.
    Revoked,
    /// Session is no longer usable.
    SessionClosed,
}

impl ErrorCode {
    /// Returns the stable wire spelling used by MCP structured errors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::IncompatibleRevision => "incompatible_revision",
            Self::Refused => "refused",
            Self::NoSuchClient => "no_such_client",
            Self::Stale => "stale",
            Self::Unsupported => "unsupported",
            Self::TimedOut => "timed_out",
            Self::InvalidArgument => "invalid_argument",
            Self::Malformed => "malformed",
            Self::TooLarge => "too_large",
            Self::Internal => "internal",
            Self::ResyncRequired => "resync_required",
            Self::Revoked => "revoked",
            Self::SessionClosed => "session_closed",
        }
    }
}

/// Machine-readable next action.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Retry {
    /// Do not retry this request unchanged.
    Never,
    /// Take a fresh observation before reconsidering.
    Reobserve,
    /// Resolve/connect/open a new provider session.
    Reconnect,
}

impl Retry {
    /// Returns the stable wire spelling used by MCP structured errors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Reobserve => "reobserve",
            Self::Reconnect => "reconnect",
        }
    }
}

/// Typed request failure. English is diagnostic, never control data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    /// Stable condition.
    pub code: ErrorCode,
    /// Stable retry action.
    pub retry: Retry,
    /// Optional exact argument field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<Name>,
    /// Optional bounded human diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Diagnostic>,
    /// Current generation for stale client results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_generation: Option<Generation>,
    /// Current sequence for stale/resync results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_sequence: Option<Sequence>,
}

impl Validate for ProtocolError {
    fn validate(&self) -> Result<(), &'static str> {
        if self.field.as_ref().is_some_and(|field| field.is_empty())
            || self
                .message
                .as_ref()
                .is_some_and(|message| message.is_empty())
        {
            return Err("error field or diagnostic is empty");
        }
        Ok(())
    }
}

/// Typed success payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Reply {
    /// Current live seat metadata.
    SeatStatus(SeatStatus),
    /// Bounded desktop snapshot.
    DesktopSnapshot(DesktopSnapshot),
    /// Initial event cursor.
    Subscribed(Subscription),
    /// Bounded event batch.
    Events(EventBatch),
    /// Terminal EWMH request observation.
    Management(ManagementReply),
    /// One application catalog page.
    Applications(ApplicationPage),
    /// Spawn result and qualified correlation.
    Launched(LaunchReply),
}

impl Validate for Reply {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::SeatStatus(value) => value.validate(),
            Self::DesktopSnapshot(value) => value.validate(),
            Self::Subscribed(value) => value.validate(),
            Self::Events(value) => value.validate(),
            Self::Management(value) => value.validate(),
            Self::Applications(value) => value.validate(),
            Self::Launched(value) => value.validate(),
        }
    }
}

/// Live session status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeatStatus {
    /// Current session identity.
    pub session: SessionId,
    /// Latest observation sequence.
    pub sequence: Sequence,
    /// Assurance carried by this status.
    pub assurance: Assurance,
}

impl Validate for SeatStatus {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// Public actions the target and manager currently advertise.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAction {
    /// Activation request.
    Activate,
    /// Polite close.
    Close,
    /// Workspace reassignment.
    ChangeWorkspace,
    /// State change.
    ChangeState,
    /// Move/resize.
    ChangeGeometry,
}

/// One visible workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDescriptor {
    /// Zero-based index.
    pub id: WorkspaceId,
    /// Optional public name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<WorkspaceName>,
    /// Public work area when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_area: Option<Rect>,
}

impl Validate for WorkspaceDescriptor {
    fn validate(&self) -> Result<(), &'static str> {
        if self.name.as_ref().is_some_and(|name| name.is_empty()) {
            return Err("workspace name is empty");
        }
        if let Some(work_area) = self.work_area {
            work_area.validate()?;
        }
        Ok(())
    }
}

/// One visible client descriptor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientDescriptor {
    /// Opaque session handle.
    pub id: ClientId,
    /// Provider-local freshness.
    pub generation: Generation,
    /// Title only when granted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Title>,
    /// Workspace when available; `None` may also mean sticky.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceId>,
    /// Public frame geometry when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Rect>,
    /// Observed states in canonical order.
    #[serde(default)]
    pub states: BoundedList<ClientState, 16>,
    /// Advertised actions in canonical order.
    #[serde(default)]
    pub actions: BoundedList<ClientAction, 8>,
}

impl Validate for ClientDescriptor {
    fn validate(&self) -> Result<(), &'static str> {
        if self.title.as_ref().is_some_and(|title| title.is_empty()) {
            return Err("client title is empty");
        }
        if let Some(frame) = self.frame {
            frame.validate()?;
        }
        if !strictly_ordered(&self.states) || !strictly_ordered(&self.actions) {
            return Err("client states and actions must be canonically ordered");
        }
        Ok(())
    }
}

/// One bounded provider observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopSnapshot {
    /// Observation cursor.
    pub sequence: Sequence,
    /// Current workspace.
    pub current_workspace: WorkspaceId,
    /// Visible workspace descriptors.
    pub workspaces: BoundedList<WorkspaceDescriptor, MAX_WORKSPACES>,
    /// Visible client descriptors.
    pub clients: BoundedList<ClientDescriptor, MAX_CLIENTS>,
    /// Active visible client, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<ClientId>,
}

impl Validate for DesktopSnapshot {
    fn validate(&self) -> Result<(), &'static str> {
        for workspace in self.workspaces.iter() {
            workspace.validate()?;
        }
        for client in self.clients.iter() {
            client.validate()?;
        }
        if !unique_by(&self.workspaces, |workspace| workspace.id)
            || !unique_by(&self.clients, |client| client.id)
        {
            return Err("snapshot identities are duplicated");
        }
        if !self
            .workspaces
            .iter()
            .any(|workspace| workspace.id == self.current_workspace)
        {
            return Err("current workspace is absent from snapshot");
        }
        if self
            .active
            .is_some_and(|active| !self.clients.iter().any(|client| client.id == active))
        {
            return Err("active client is absent from snapshot");
        }
        Ok(())
    }
}

/// Initial event subscription result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    /// Cursor after the initial synchronized state.
    pub cursor: Sequence,
}

impl Validate for Subscription {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// One sequenced visible change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    /// Monotonic session sequence.
    pub sequence: Sequence,
    /// Typed visible change.
    pub event: Event,
}

impl Validate for EventEnvelope {
    fn validate(&self) -> Result<(), &'static str> {
        self.event.validate()
    }
}

/// Visible event payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Event {
    /// Client appeared.
    ClientAdded(ClientDescriptor),
    /// Visible client descriptor changed.
    ClientChanged(ClientDescriptor),
    /// Client disappeared.
    ClientRemoved(ClientId),
    /// Active visible client changed.
    ActiveChanged(Option<ClientId>),
    /// Workspace facts changed; take a snapshot for full state.
    WorkspaceChanged(WorkspaceId),
    /// Application catalog changed; relist from cursor zero.
    ApplicationsChanged(Empty),
}

impl Validate for Event {
    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::ClientAdded(client) | Self::ClientChanged(client) => client.validate(),
            Self::ClientRemoved(_)
            | Self::ActiveChanged(_)
            | Self::WorkspaceChanged(_)
            | Self::ApplicationsChanged(_) => Ok(()),
        }
    }
}

/// Bounded event poll response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventBatch {
    /// Events after the requested cursor.
    pub events: BoundedList<EventEnvelope, MAX_EVENTS>,
    /// Cursor represented by this result.
    pub cursor: Sequence,
}

impl Validate for EventBatch {
    fn validate(&self) -> Result<(), &'static str> {
        let mut prior = None;
        for event in self.events.iter() {
            event.validate()?;
            if prior.is_some_and(|prior| event.sequence <= prior) || event.sequence > self.cursor {
                return Err("event sequences are not strictly increasing within the cursor");
            }
            prior = Some(event.sequence);
        }
        Ok(())
    }
}

/// Post-request observation for a sent EWMH message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    /// Desired public state was observed.
    Observed,
    /// Desired public state was not observed before the fixed deadline.
    TimedOut,
    /// Target disappeared after the message was sent; outcome is unknown.
    TargetGone,
}

/// Result returned only after an EWMH request was sent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagementReply {
    /// Terminal public observation.
    pub observation: Observation,
    /// Observation cursor after the attempt.
    pub sequence: Sequence,
}

impl Validate for ManagementReply {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// One policy-visible desktop application.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationDescriptor {
    /// Canonical desktop ID.
    pub id: ApplicationId,
    /// Localized display name.
    pub name: Name,
    /// Whether the winning entry is user-writable.
    pub user_entry: bool,
}

impl Validate for ApplicationDescriptor {
    fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_empty() || !self.id.ends_with(".desktop") || self.name.is_empty() {
            return Err("application descriptor is not canonical");
        }
        Ok(())
    }
}

/// One bounded application catalog page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationPage {
    /// Entries in canonical desktop-ID order.
    pub applications: BoundedList<ApplicationDescriptor, MAX_APPLICATIONS>,
    /// Next opaque index, absent at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u32>,
}

impl Validate for ApplicationPage {
    fn validate(&self) -> Result<(), &'static str> {
        for application in self.applications.iter() {
            application.validate()?;
        }
        if !self
            .applications
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
        {
            return Err("application IDs must be unique and canonically ordered");
        }
        Ok(())
    }
}

/// Controlled launch result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchReply {
    /// Unique provider launch identity.
    pub token: LaunchToken,
    /// Best-effort correlated visible client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientId>,
}

impl Validate for LaunchReply {
    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

fn strictly_ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn unique_by<T, K: Eq + Copy>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, value)| values[..index].iter().all(|prior| key(prior) != key(value)))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;

    fn text<const N: usize>(value: &str) -> BoundedText<N> {
        BoundedText::new(value).expect("bounded fixture")
    }

    fn peer() -> PeerInfo {
        PeerInfo {
            name: text("test-peer"),
            version: text("0.1.0"),
            purpose: text("strict wire test"),
        }
    }

    #[test]
    fn incompatible_hello_remains_decodable_for_precise_refusal() {
        let hello = Hello {
            protocol: text(PROTOCOL_NAME),
            revision: PROTOCOL_REVISION + 1,
            peer: peer(),
            requested: BoundedList::default(),
        };
        assert!(hello.validate().is_ok());
        assert!(!hello.is_compatible());
    }

    #[test]
    fn unknown_message_and_argument_fields_are_rejected() {
        let unknown_outer = r#"{"type":"hello","body":{"protocol":"agent-seat","revision":3,"peer":{"name":"p","version":"1","purpose":"t"},"requested":[]},"extra":1}"#;
        let unknown_argument = r#"{"type":"request","body":{"id":1,"call":{"name":"seat.status","arguments":{"extra":1}}}}"#;
        assert!(serde_json::from_str::<ClientMessage>(unknown_outer).is_err());
        assert!(serde_json::from_str::<ClientMessage>(unknown_argument).is_err());
    }

    #[test]
    fn capability_order_is_canonical_and_duplicate_free() {
        let hello = Hello {
            protocol: text(PROTOCOL_NAME),
            revision: PROTOCOL_REVISION,
            peer: peer(),
            requested: BoundedList::new(vec![
                Capability::ManageClose,
                Capability::ObserveStructure,
            ])
            .expect("bounded fixture"),
        };
        assert!(hello.validate().is_err());
    }

    #[test]
    fn request_ids_are_nonzero_at_deserialization() {
        let zero =
            r#"{"type":"request","body":{"id":0,"call":{"name":"seat.status","arguments":{}}}}"#;
        assert!(serde_json::from_str::<ClientMessage>(zero).is_err());
        let id = RequestId::new(NonZeroU64::new(1).expect("nonzero"));
        assert_eq!(id.get(), 1);
    }

    #[test]
    fn snapshots_reject_dangling_active_clients() {
        let snapshot = DesktopSnapshot {
            sequence: Sequence::new(1),
            current_workspace: WorkspaceId::new(0),
            workspaces: BoundedList::new(vec![WorkspaceDescriptor {
                id: WorkspaceId::new(0),
                name: None,
                work_area: None,
            }])
            .expect("bounded fixture"),
            clients: BoundedList::default(),
            active: Some(ClientId::new(NonZeroU64::new(1).expect("nonzero"))),
        };
        assert!(snapshot.validate().is_err());
    }

    #[test]
    fn advertised_limits_cannot_exceed_revision_bounds() {
        let valid = Limits {
            request_frame_bytes: MAX_REQUEST_FRAME_BYTES as u32,
            response_frame_bytes: MAX_RESPONSE_FRAME_BYTES as u32,
            events_per_poll: MAX_EVENTS as u16,
            poll_wait_ms: MAX_POLL_WAIT_MS,
        };
        assert!(valid.validate().is_ok());
        assert!(
            Limits {
                request_frame_bytes: valid.request_frame_bytes + 1,
                ..valid
            }
            .validate()
            .is_err()
        );
        assert!(
            Limits {
                response_frame_bytes: valid.response_frame_bytes + 1,
                ..valid
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn application_ids_are_canonical_desktop_ids() {
        for application in ["", "org.example.App", "nested/org.example.App.desktop"] {
            assert!(
                ApplicationLaunchRequest {
                    application: text(application),
                }
                .validate()
                .is_err(),
                "accepted {application:?}"
            );
        }
        assert!(
            ApplicationLaunchRequest {
                application: text("org.example.App.desktop"),
            }
            .validate()
            .is_ok()
        );
    }
}
