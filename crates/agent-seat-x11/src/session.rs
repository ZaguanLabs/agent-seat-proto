//! Bounded authenticated wire sessions with provider-owned grants.

use std::num::NonZeroU64;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use agent_seat_proto::{
    Assurance, Backend, BoundedList, BoundedText, Call, ClientMessage, Diagnostic, ErrorCode,
    Feature, Goodbye, Limits, MAX_EVENTS, MAX_POLL_WAIT_MS, MAX_REQUEST_FRAME_BYTES,
    MAX_RESPONSE_FRAME_BYTES, Outcome, PROTOCOL_NAME, PROTOCOL_REVISION, ProtocolError,
    ProviderInfo, ReadFrame, Reply, Request, Response, Retry, Sequence, ServerMessage, SessionId,
    Welcome, read_frame, write_frame,
};
use rustix::net::sockopt::socket_peercred;

use crate::config::Config;
use crate::observer::{Failure as ObservationFailure, Observer, Operation};

pub(crate) fn run(
    mut stream: UnixStream,
    config: Arc<Config>,
    session_number: NonZeroU64,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(config.io_timeout()))
        .and_then(|()| stream.set_write_timeout(Some(config.io_timeout())))
        .map_err(|error| format!("cannot bound session I/O: {error}"))?;
    let credentials = socket_peercred(&stream)
        .map_err(|error| format!("cannot authenticate local peer credentials: {error}"))?;
    let uid = credentials.uid.as_raw();

    let hello = match read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES)
        .map_err(|error| format!("cannot read session hello: {error}"))?
    {
        ReadFrame::Message(ClientMessage::Hello(hello)) => hello,
        ReadFrame::Message(_) => {
            return close(&mut stream, ErrorCode::Malformed, "hello must be first");
        }
        ReadFrame::CleanEof => return Ok(()),
    };
    if !hello.is_compatible() {
        return close(
            &mut stream,
            ErrorCode::IncompatibleRevision,
            "exact Agent Seat revision 3 is required",
        );
    }
    let Some(granted) = config.granted(uid, hello.requested.iter()) else {
        return close(
            &mut stream,
            ErrorCode::Refused,
            "verified peer UID has no configured grant",
        );
    };
    let granted = BoundedList::new(granted)
        .map_err(|error| format!("configured grant exceeded wire bounds: {error}"))?;
    let session = SessionId::new(session_number);
    let welcome = Welcome {
        protocol: text::<128>(PROTOCOL_NAME)?,
        revision: PROTOCOL_REVISION,
        session,
        provider: ProviderInfo {
            name: text("agent-seat-x11")?,
            version: text(env!("CARGO_PKG_VERSION"))?,
        },
        backend: Backend::X11Ewmh,
        assurance: Assurance::Tier0,
        features: BoundedList::new(vec![Feature::EwmhObservation, Feature::EwmhManagement])
            .map_err(|error| format!("provider feature list exceeded wire bounds: {error}"))?,
        granted: granted.clone(),
        limits: Limits {
            request_frame_bytes: MAX_REQUEST_FRAME_BYTES as u32,
            response_frame_bytes: MAX_RESPONSE_FRAME_BYTES as u32,
            events_per_poll: MAX_EVENTS as u16,
            poll_wait_ms: MAX_POLL_WAIT_MS,
        },
    };
    write_frame(
        &mut stream,
        &ServerMessage::Welcome(welcome),
        MAX_RESPONSE_FRAME_BYTES,
    )
    .map_err(|error| format!("cannot write session welcome: {error}"))?;

    let mut observer = None;
    for _ in 0..config.max_requests() {
        let message = match read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES)
            .map_err(|error| format!("cannot read session request: {error}"))?
        {
            ReadFrame::Message(message) => message,
            ReadFrame::CleanEof => return Ok(()),
        };
        match message {
            ClientMessage::Request(request) => {
                let response = handle(request, session, &granted, &config, &mut observer);
                write_frame(
                    &mut stream,
                    &ServerMessage::Response(response),
                    MAX_RESPONSE_FRAME_BYTES,
                )
                .map_err(|error| format!("cannot write session response: {error}"))?;
            }
            ClientMessage::Goodbye(_) => return Ok(()),
            ClientMessage::Hello(_) => {
                return close(
                    &mut stream,
                    ErrorCode::Malformed,
                    "hello cannot be repeated",
                );
            }
        }
    }
    close(
        &mut stream,
        ErrorCode::SessionClosed,
        "session request bound reached",
    )
}

pub(crate) fn reject_capacity(mut stream: UnixStream) {
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(100)));
    let _ = close(
        &mut stream,
        ErrorCode::Unavailable,
        "provider session limit reached",
    );
}

fn handle(
    request: Request,
    session: SessionId,
    granted: &[agent_seat_proto::Capability],
    config: &Config,
    observer: &mut Option<Observer>,
) -> Response {
    let outcome = if !authorized(&request.call, granted) {
        protocol_error(
            ErrorCode::Refused,
            Retry::Never,
            "capability was not granted",
        )
    } else {
        observe_call(request.call, session, granted, config, observer)
    };
    Response {
        id: request.id,
        outcome,
    }
}

fn authorized(call: &Call, granted: &[agent_seat_proto::Capability]) -> bool {
    granted.contains(&call.required_capability())
        && (!matches!(
            call,
            Call::ClientActivate(_)
                | Call::ClientClose(_)
                | Call::WorkspaceSwitch(_)
                | Call::ClientWorkspace(_)
                | Call::ClientState(_)
                | Call::ClientGeometry(_)
        ) || granted.contains(&agent_seat_proto::Capability::ObserveStructure))
}

fn observe_call(
    call: Call,
    session: SessionId,
    granted: &[agent_seat_proto::Capability],
    config: &Config,
    observer: &mut Option<Observer>,
) -> Outcome {
    let result = match call {
        Call::SeatStatus(_) => {
            return Outcome::Ok(Reply::SeatStatus(agent_seat_proto::SeatStatus {
                session,
                sequence: observer
                    .as_ref()
                    .map_or(Sequence::new(0), Observer::sequence),
                assurance: Assurance::Tier0,
            }));
        }
        Call::DesktopSnapshot(_) => observer_for(observer, granted, config)
            .and_then(Observer::snapshot)
            .map(Reply::DesktopSnapshot),
        Call::EventsSubscribe(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.subscribe(&arguments.kinds))
            .map(Reply::Subscribed),
        Call::EventsPoll(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.poll(arguments.after, arguments.limit, arguments.wait_ms))
            .map(Reply::Events),
        Call::ClientActivate(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::Activate(arguments)))
            .map(Reply::Management),
        Call::ClientClose(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::Close(arguments)))
            .map(Reply::Management),
        Call::WorkspaceSwitch(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::WorkspaceSwitch(arguments)))
            .map(Reply::Management),
        Call::ClientWorkspace(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::ClientWorkspace(arguments)))
            .map(Reply::Management),
        Call::ClientState(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::State(arguments)))
            .map(Reply::Management),
        Call::ClientGeometry(arguments) => observer_for(observer, granted, config)
            .and_then(|observer| observer.manage(Operation::Geometry(arguments)))
            .map(Reply::Management),
        _ => {
            return protocol_error(
                ErrorCode::Unsupported,
                Retry::Never,
                "operation is not implemented by the T1 observer",
            );
        }
    };
    match result {
        Ok(reply) => Outcome::Ok(reply),
        Err(error) => observation_error(error),
    }
}

fn observer_for<'a>(
    observer: &'a mut Option<Observer>,
    granted: &[agent_seat_proto::Capability],
    config: &Config,
) -> Result<&'a mut Observer, ObservationFailure> {
    if observer.is_none() {
        let show_titles = config.titles_enabled()
            && granted.contains(&agent_seat_proto::Capability::ObserveTitles);
        let connected = Observer::connect(config.client_scope(), show_titles)?;
        *observer = Some(connected);
    }
    observer
        .as_mut()
        .ok_or_else(|| ObservationFailure::internal("observer initialization failed"))
}

fn observation_error(error: ObservationFailure) -> Outcome {
    Outcome::Error(ProtocolError {
        code: error.code,
        retry: error.retry,
        field: None,
        message: Some(diagnostic(error.message)),
        current_generation: error.current_generation,
        current_sequence: error.current_sequence,
    })
}

fn protocol_error(code: ErrorCode, retry: Retry, message: &str) -> Outcome {
    Outcome::Error(ProtocolError {
        code,
        retry,
        field: None,
        message: Some(diagnostic(message)),
        current_generation: None,
        current_sequence: None,
    })
}

fn close(stream: &mut UnixStream, code: ErrorCode, message: &str) -> Result<(), String> {
    write_frame(
        stream,
        &ServerMessage::Goodbye(Goodbye {
            code,
            message: Some(diagnostic(message)),
        }),
        MAX_RESPONSE_FRAME_BYTES,
    )
    .map_err(|error| format!("cannot write terminal session message: {error}"))
}

fn text<const N: usize>(value: &str) -> Result<BoundedText<N>, String> {
    BoundedText::new(value).map_err(|error| error.to_string())
}

fn diagnostic(value: &str) -> Diagnostic {
    Diagnostic::new(value).expect("static provider diagnostics fit their public bound")
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use agent_seat_proto::{Empty, RequestId};

    use super::*;

    #[test]
    fn calls_are_rechecked_against_the_grant() {
        let request = Request {
            id: RequestId::new(NonZeroU64::MIN),
            call: Call::SeatStatus(Empty {}),
        };
        assert!(!authorized(&request.call, &[]));
        assert!(authorized(
            &request.call,
            &[agent_seat_proto::Capability::ObserveStructure]
        ));

        let close = Call::ClientClose(agent_seat_proto::TargetRequest {
            client: agent_seat_proto::ClientId::new(NonZeroU64::MIN),
            generation: agent_seat_proto::Generation::new(0),
        });
        assert!(!authorized(
            &close,
            &[agent_seat_proto::Capability::ManageClose]
        ));
        assert!(authorized(
            &close,
            &[
                agent_seat_proto::Capability::ObserveStructure,
                agent_seat_proto::Capability::ManageClose,
            ]
        ));
    }
}
