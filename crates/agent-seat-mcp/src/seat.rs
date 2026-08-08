//! Lazy provider connection and strict request/response pairing.

use std::num::NonZeroU64;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use agent_seat_proto::{
    BoundedList, BoundedText, Call, Capability, ClientMessage, CodecError, ErrorCode, Goodbye,
    Hello, MAX_REQUEST_FRAME_BYTES, MAX_RESPONSE_FRAME_BYTES, PROTOCOL_NAME, PROTOCOL_REVISION,
    PeerInfo, ReadFrame, Request, RequestId, Response, Retry, ServerMessage, Validate, read_frame,
    write_frame,
};

const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct Seat {
    stream: UnixStream,
    next_request: NonZeroU64,
}

#[derive(Debug)]
pub(crate) struct SeatError {
    code: ErrorCode,
    retry: Retry,
    message: String,
}

impl SeatError {
    pub(crate) const fn code(&self) -> &'static str {
        self.code.as_str()
    }

    pub(crate) const fn retry(&self) -> &'static str {
        self.retry.as_str()
    }
}

impl std::fmt::Display for SeatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SeatError {}

impl Seat {
    pub(crate) fn connect(path: &Path) -> Result<Self, SeatError> {
        let mut stream = UnixStream::connect(path).map_err(|error| {
            unavailable(format!("cannot connect to {}: {error}", path.display()))
        })?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|error| unavailable(format!("cannot bound provider I/O: {error}")))?;

        let requested = BoundedList::new(vec![
            Capability::ObserveStructure,
            Capability::ObserveTitles,
            Capability::ObserveEvents,
            Capability::ManageActivate,
            Capability::ManageClose,
            Capability::ManageWorkspace,
            Capability::ManageState,
            Capability::ManageGeometry,
            Capability::LaunchList,
            Capability::LaunchExecute,
            Capability::InputPointer,
        ])
        .map_err(|error| internal(error.to_string()))?;
        let hello = ClientMessage::Hello(Hello {
            protocol: text(PROTOCOL_NAME)?,
            revision: PROTOCOL_REVISION,
            peer: PeerInfo {
                name: text("agent-seat-mcp")?,
                version: text(env!("CARGO_PKG_VERSION"))?,
                purpose: text("translate MCP desktop tools")?,
            },
            requested,
        });
        write_frame(&mut stream, &hello, MAX_REQUEST_FRAME_BYTES).map_err(codec_error)?;
        let welcome =
            match read_frame(&mut stream, MAX_RESPONSE_FRAME_BYTES).map_err(codec_error)? {
                ReadFrame::Message(ServerMessage::Welcome(welcome)) => welcome,
                ReadFrame::Message(ServerMessage::Goodbye(goodbye)) => {
                    return Err(goodbye_error("provider refused the session", goodbye));
                }
                ReadFrame::Message(ServerMessage::Response(_)) => {
                    return Err(malformed("provider responded before opening the session"));
                }
                ReadFrame::CleanEof => {
                    return Err(unavailable("provider closed during session opening"));
                }
            };
        welcome.validate().map_err(malformed)?;
        Ok(Self {
            stream,
            next_request: NonZeroU64::MIN,
        })
    }

    pub(crate) fn call(&mut self, call: Call) -> Result<Response, SeatError> {
        let id = RequestId::new(self.next_request);
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or_else(|| internal("provider request identity space is exhausted"))?;
        let message = ClientMessage::Request(Request { id, call });
        write_frame(&mut self.stream, &message, MAX_REQUEST_FRAME_BYTES).map_err(codec_error)?;
        match read_frame(&mut self.stream, MAX_RESPONSE_FRAME_BYTES).map_err(codec_error)? {
            ReadFrame::Message(ServerMessage::Response(response)) if response.id == id => {
                Ok(response)
            }
            ReadFrame::Message(ServerMessage::Response(_)) => Err(malformed(
                "provider response identity does not match the request",
            )),
            ReadFrame::Message(ServerMessage::Goodbye(goodbye)) => {
                Err(goodbye_error("provider closed the session", goodbye))
            }
            ReadFrame::Message(ServerMessage::Welcome(_)) => {
                Err(malformed("provider repeated the opening response"))
            }
            ReadFrame::CleanEof => Err(unavailable("provider closed before responding")),
        }
    }
}

fn unavailable(message: impl Into<String>) -> SeatError {
    SeatError {
        code: ErrorCode::Unavailable,
        retry: Retry::Reconnect,
        message: message.into(),
    }
}

fn malformed(message: impl Into<String>) -> SeatError {
    SeatError {
        code: ErrorCode::Malformed,
        retry: Retry::Reconnect,
        message: message.into(),
    }
}

fn internal(message: impl Into<String>) -> SeatError {
    SeatError {
        code: ErrorCode::Internal,
        retry: Retry::Never,
        message: message.into(),
    }
}

fn codec_error(error: CodecError) -> SeatError {
    match error {
        CodecError::Io(error) => unavailable(format!("provider I/O failed: {error}")),
        CodecError::TooLarge { .. } => SeatError {
            code: ErrorCode::TooLarge,
            retry: Retry::Never,
            message: error.to_string(),
        },
        _ => malformed(error.to_string()),
    }
}

fn goodbye_error(context: &str, goodbye: Goodbye) -> SeatError {
    let retry = match goodbye.code {
        ErrorCode::NoSuchClient
        | ErrorCode::Stale
        | ErrorCode::TimedOut
        | ErrorCode::ResyncRequired => Retry::Reobserve,
        ErrorCode::Unavailable
        | ErrorCode::Malformed
        | ErrorCode::Internal
        | ErrorCode::Revoked
        | ErrorCode::SessionClosed => Retry::Reconnect,
        ErrorCode::IncompatibleRevision
        | ErrorCode::Refused
        | ErrorCode::Unsupported
        | ErrorCode::InvalidArgument
        | ErrorCode::TooLarge => Retry::Never,
    };
    let diagnostic = goodbye
        .message
        .as_ref()
        .map_or_else(String::new, |message| format!(": {message}"));
    SeatError {
        code: goodbye.code,
        retry,
        message: format!("{context}: {}{diagnostic}", goodbye.code.as_str()),
    }
}

fn text<const N: usize>(value: &str) -> Result<BoundedText<N>, SeatError> {
    BoundedText::new(value).map_err(|error| internal(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::thread;

    use agent_seat_proto::{ErrorCode, Goodbye};

    use super::*;

    #[test]
    fn provider_revision_refusal_is_terminal() {
        let path =
            std::env::temp_dir().join(format!("agent-seat-e1-refusal-{}.sock", std::process::id()));
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind test provider");
        let provider = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept companion");
            assert!(matches!(
                read_frame(&mut stream, MAX_REQUEST_FRAME_BYTES).expect("read hello"),
                ReadFrame::Message(ClientMessage::Hello(hello)) if hello.is_compatible()
            ));
            write_frame(
                &mut stream,
                &ServerMessage::Goodbye(Goodbye {
                    code: ErrorCode::IncompatibleRevision,
                    message: None,
                }),
                MAX_RESPONSE_FRAME_BYTES,
            )
            .expect("write refusal");
        });
        let error = Seat::connect(&path)
            .err()
            .expect("revision refusal must not open a session");
        provider.join().expect("provider thread");
        fs::remove_file(&path).expect("remove test socket");
        assert_eq!(error.code(), "incompatible_revision");
        assert_eq!(error.retry(), "never");
    }
}
