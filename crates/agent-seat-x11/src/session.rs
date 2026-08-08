//! Bounded authenticated wire sessions with provider-owned grants.

use std::num::NonZeroU64;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use agent_seat_proto::{
    Assurance, Backend, BoundedList, BoundedText, Call, ClientMessage, Diagnostic, ErrorCode,
    Goodbye, Limits, MAX_EVENTS, MAX_REQUEST_FRAME_BYTES, MAX_RESPONSE_FRAME_BYTES, Outcome,
    PROTOCOL_NAME, PROTOCOL_REVISION, ProtocolError, ProviderInfo, ReadFrame, Reply, Request,
    Response, Retry, Sequence, ServerMessage, SessionId, Welcome, read_frame, write_frame,
};
use rustix::net::sockopt::socket_peercred;

use crate::config::Config;

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
        features: BoundedList::default(),
        granted: granted.clone(),
        limits: Limits {
            request_frame_bytes: MAX_REQUEST_FRAME_BYTES as u32,
            response_frame_bytes: MAX_RESPONSE_FRAME_BYTES as u32,
            events_per_poll: MAX_EVENTS as u16,
            poll_wait_ms: 0,
        },
    };
    write_frame(
        &mut stream,
        &ServerMessage::Welcome(welcome),
        MAX_RESPONSE_FRAME_BYTES,
    )
    .map_err(|error| format!("cannot write session welcome: {error}"))?;

    for _ in 0..config.max_requests() {
        let message = match read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES)
            .map_err(|error| format!("cannot read session request: {error}"))?
        {
            ReadFrame::Message(message) => message,
            ReadFrame::CleanEof => return Ok(()),
        };
        match message {
            ClientMessage::Request(request) => {
                let response = handle(request, session, &granted);
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
) -> Response {
    let outcome = if !granted.contains(&request.call.required_capability()) {
        protocol_error(
            ErrorCode::Refused,
            Retry::Never,
            "capability was not granted",
        )
    } else {
        match request.call {
            Call::SeatStatus(_) => Outcome::Ok(Reply::SeatStatus(agent_seat_proto::SeatStatus {
                session,
                sequence: Sequence::new(0),
                assurance: Assurance::Tier0,
            })),
            _ => protocol_error(
                ErrorCode::Unsupported,
                Retry::Never,
                "operation is not implemented by the T0 foundation",
            ),
        }
    };
    Response {
        id: request.id,
        outcome,
    }
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
        assert!(matches!(
            handle(request.clone(), SessionId::new(NonZeroU64::MIN), &[]).outcome,
            Outcome::Error(ProtocolError {
                code: ErrorCode::Refused,
                ..
            })
        ));
        assert!(matches!(
            handle(
                request,
                SessionId::new(NonZeroU64::MIN),
                &[agent_seat_proto::Capability::ObserveStructure]
            )
            .outcome,
            Outcome::Ok(Reply::SeatStatus(_))
        ));
    }
}
