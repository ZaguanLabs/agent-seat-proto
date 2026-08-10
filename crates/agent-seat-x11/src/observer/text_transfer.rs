//! Request-local write-only X11 selection transfer for one focused target.

use std::time::{Duration, Instant};

use agent_seat_proto::{
    KeyboardKey, KeyboardModifier, TextInsertRequest, TextTransferReply, TextTransferTerminal,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::CURRENT_TIME;
use x11rb::NONE;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::keyboard::resolve_key;
use super::{Failure, Observer};
use crate::seat::{SeatGate, SeatPermit};

const TRANSFER_WAIT: Duration = Duration::from_secs(2);
const EVIDENCE_INTERVAL: Duration = Duration::from_millis(50);
const MAX_TRANSFER_EVENTS: usize = 256;
const MAX_SELECTION_REQUESTS: usize = 32;

impl Observer {
    pub(crate) fn text_insert(
        &mut self,
        request: TextInsertRequest,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<TextTransferReply, Failure> {
        let target_request = request.target;
        let text = request.text.into_string();
        let requested_bytes = u32::try_from(text.len())
            .map_err(|_| Failure::internal("text-transfer byte count cannot be represented"))?;
        let owner = self.create_transfer_owner()?;
        let offered = self.under_server_grab(|observer| {
            observer.refresh()?;
            let target = observer.target(target_request)?;
            observer.require_focus_owned_by(target.xid)?;
            observer.require_xres_client_identity(target.xid)?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            observer.claim_transfer_selection(owner)?;
            let stroke = resolve_key(
                &observer.connection,
                KeyboardKey::V,
                &[KeyboardModifier::Control],
            )?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            observer.type_key(&stroke)?;
            Ok(true)
        });
        let result = match offered {
            Ok(true) => self.wait_for_transfer(
                owner,
                target_request,
                text.as_bytes(),
                requested_bytes,
                seat,
                seat_permit,
            ),
            Ok(false) => Ok(transfer_reply(
                target_request,
                requested_bytes,
                TextTransferTerminal::Interrupted,
            )),
            Err(error) => Err(error),
        };
        let cleanup = self.release_transfer_owner(owner);
        match (result, cleanup) {
            (Ok(reply), Ok(())) => Ok(reply),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    fn create_transfer_owner(&self) -> Result<u32, Failure> {
        let owner = self
            .connection
            .generate_id()
            .map_err(|_| Failure::unavailable("cannot allocate a text-transfer owner window"))?;
        self.connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                owner,
                self.root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .map_err(|_| Failure::unavailable("cannot create a text-transfer owner window"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot create a text-transfer owner window"))?;
        Ok(owner)
    }

    fn claim_transfer_selection(&self, owner: u32) -> Result<(), Failure> {
        self.connection
            .set_selection_owner(owner, self.atoms.clipboard, CURRENT_TIME)
            .map_err(|_| Failure::unavailable("cannot claim the X11 clipboard selection"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot claim the X11 clipboard selection"))?;
        let current = self
            .connection
            .get_selection_owner(self.atoms.clipboard)
            .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
            .owner;
        if current != owner {
            return Err(Failure::unavailable(
                "X11 clipboard ownership changed before text transfer",
            ));
        }
        Ok(())
    }

    fn wait_for_transfer(
        &mut self,
        owner: u32,
        target_request: agent_seat_proto::TargetRequest,
        text: &[u8],
        requested_bytes: u32,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<TextTransferReply, Failure> {
        let deadline = Instant::now()
            .checked_add(TRANSFER_WAIT)
            .ok_or_else(|| Failure::internal("text-transfer deadline overflowed"))?;
        let mut events_seen = 0_usize;
        let mut selection_requests = 0_usize;
        loop {
            while let Some(event) = self
                .connection
                .poll_for_event()
                .map_err(|_| Failure::unavailable("cannot read X11 text-transfer events"))?
            {
                events_seen = events_seen.saturating_add(1);
                if events_seen > MAX_TRANSFER_EVENTS {
                    return Ok(transfer_reply(
                        target_request,
                        requested_bytes,
                        TextTransferTerminal::Interrupted,
                    ));
                }
                match event {
                    Event::SelectionClear(event)
                        if event.owner == owner && event.selection == self.atoms.clipboard =>
                    {
                        return Ok(transfer_reply(
                            target_request,
                            requested_bytes,
                            TextTransferTerminal::Interrupted,
                        ));
                    }
                    Event::SelectionRequest(event)
                        if event.owner == owner && event.selection == self.atoms.clipboard =>
                    {
                        selection_requests = selection_requests.saturating_add(1);
                        if selection_requests > MAX_SELECTION_REQUESTS {
                            return Ok(transfer_reply(
                                target_request,
                                requested_bytes,
                                TextTransferTerminal::Interrupted,
                            ));
                        }
                        if self.handle_selection_request(
                            event,
                            owner,
                            target_request,
                            text,
                            seat,
                            seat_permit,
                        )? {
                            return Ok(TextTransferReply {
                                target: target_request,
                                requested_bytes,
                                delivered_bytes: requested_bytes,
                                terminal: TextTransferTerminal::Delivered,
                            });
                        }
                    }
                    _ => {}
                }
            }

            if Instant::now() >= deadline {
                return Ok(transfer_reply(
                    target_request,
                    requested_bytes,
                    TextTransferTerminal::Offered,
                ));
            }
            if !self.transfer_evidence_is_current(owner, target_request, seat, seat_permit) {
                return Ok(transfer_reply(
                    target_request,
                    requested_bytes,
                    TextTransferTerminal::Interrupted,
                ));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = remaining.min(EVIDENCE_INTERVAL);
            let timeout = Timespec {
                tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(wait.subsec_nanos()),
            };
            let mut polls = [PollFd::new(self.connection.stream(), PollFlags::IN)];
            poll(&mut polls, Some(&timeout))
                .map_err(|_| Failure::unavailable("cannot wait for X11 text-transfer events"))?;
        }
    }

    fn transfer_evidence_is_current(
        &mut self,
        owner: u32,
        target_request: agent_seat_proto::TargetRequest,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> bool {
        self.under_server_grab(|observer| {
            observer.refresh()?;
            let target = observer.target(target_request)?;
            observer.require_focus_owned_by(target.xid)?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            let current = observer
                .connection
                .get_selection_owner(observer.atoms.clipboard)
                .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
                .owner;
            Ok(current == owner)
        })
        .unwrap_or(false)
    }

    fn handle_selection_request(
        &mut self,
        request: SelectionRequestEvent,
        owner: u32,
        target_request: agent_seat_proto::TargetRequest,
        text: &[u8],
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<bool, Failure> {
        self.under_server_grab(|observer| {
            observer.refresh()?;
            let target = observer.target(target_request)?;
            observer.require_focus_owned_by(target.xid)?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            if !observer.same_x_client(target.xid, request.requestor)? {
                observer.notify_selection(request, NONE)?;
                observer.connection.sync().map_err(|_| {
                    Failure::unavailable("cannot refuse an out-of-scope selection requestor")
                })?;
                return Ok(false);
            }
            let current = observer
                .connection
                .get_selection_owner(observer.atoms.clipboard)
                .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot verify X11 clipboard ownership"))?
                .owner;
            if current != owner {
                return Ok(false);
            }
            let property = if request.property == NONE {
                request.target
            } else {
                request.property
            };
            if request.target == observer.atoms.targets {
                let targets = [
                    observer.atoms.targets,
                    observer.atoms.utf8,
                    observer.atoms.text_plain_utf8,
                    observer.atoms.text_plain,
                ];
                observer
                    .connection
                    .change_property32(
                        PropMode::REPLACE,
                        request.requestor,
                        property,
                        AtomEnum::ATOM,
                        &targets,
                    )
                    .map_err(|_| Failure::unavailable("cannot publish text-transfer targets"))?
                    .check()
                    .map_err(|_| Failure::unavailable("cannot publish text-transfer targets"))?;
                observer.notify_selection(request, property)?;
                observer.connection.sync().map_err(|_| {
                    Failure::unavailable("cannot synchronize text-transfer targets")
                })?;
                return Ok(false);
            }
            if !matches!(
                request.target,
                target
                    if target == observer.atoms.utf8
                        || target == observer.atoms.text_plain_utf8
                        || target == observer.atoms.text_plain
            ) {
                observer.notify_selection(request, NONE)?;
                observer.connection.sync().map_err(|_| {
                    Failure::unavailable("cannot refuse an unsupported text target")
                })?;
                return Ok(false);
            }
            observer
                .connection
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    request.target,
                    text,
                )
                .map_err(|_| Failure::unavailable("cannot deliver UTF-8 transfer text"))?
                .check()
                .map_err(|_| Failure::unavailable("cannot deliver UTF-8 transfer text"))?;
            observer.notify_selection(request, property)?;
            observer
                .connection
                .sync()
                .map_err(|_| Failure::unavailable("cannot synchronize UTF-8 text delivery"))?;
            Ok(true)
        })
    }

    fn notify_selection(
        &self,
        request: SelectionRequestEvent,
        property: u32,
    ) -> Result<(), Failure> {
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property,
        };
        self.connection
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
            .map_err(|_| Failure::unavailable("cannot send X11 selection notification"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot send X11 selection notification"))
    }

    fn require_xres_client_identity(&self, window: u32) -> Result<u32, Failure> {
        let version = self
            .connection
            .res_query_version(1, 2)
            .map_err(|_| Failure::unavailable("X-Resource client identity is unavailable"))?
            .reply()
            .map_err(|_| Failure::unavailable("X-Resource client identity is unavailable"))?;
        if version.server_major < 1 || (version.server_major == 1 && version.server_minor < 2) {
            return Err(Failure::unavailable(
                "X-Resource 1.2 client identity is unavailable",
            ));
        }
        let spec = [ClientIdSpec {
            client: window,
            mask: ClientIdMask::CLIENT_XID,
        }];
        let reply = self
            .connection
            .res_query_client_ids(&spec)
            .map_err(|_| Failure::unavailable("cannot inspect X11 client identity"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect X11 client identity"))?;
        let value = reply
            .ids
            .iter()
            .find(|value| value.spec.mask == ClientIdMask::CLIENT_XID && value.value.is_empty())
            .map(|value| value.spec.client)
            .ok_or_else(|| Failure::unavailable("X11 client identity evidence is incomplete"))?;
        Ok(value)
    }

    fn same_x_client(&self, target: u32, requestor: u32) -> Result<bool, Failure> {
        let target_client = self.require_xres_client_identity(target)?;
        let requestor_client = self.require_xres_client_identity(requestor)?;
        Ok(target_client == requestor_client)
    }

    fn release_transfer_owner(&self, owner: u32) -> Result<(), Failure> {
        self.connection
            .grab_server()
            .map_err(|_| Failure::unavailable("cannot acquire text-transfer cleanup grab"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot acquire text-transfer cleanup grab"))?;
        let cleanup = (|| {
            let current = self
                .connection
                .get_selection_owner(self.atoms.clipboard)
                .map_err(|_| Failure::unavailable("cannot inspect text-transfer cleanup state"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot inspect text-transfer cleanup state"))?
                .owner;
            if current == owner {
                self.connection
                    .set_selection_owner(NONE, self.atoms.clipboard, CURRENT_TIME)
                    .map_err(|_| Failure::unavailable("cannot release X11 clipboard ownership"))?
                    .check()
                    .map_err(|_| Failure::unavailable("cannot release X11 clipboard ownership"))?;
            }
            self.connection
                .destroy_window(owner)
                .map_err(|_| Failure::unavailable("cannot destroy text-transfer owner window"))?
                .check()
                .map_err(|_| Failure::unavailable("cannot destroy text-transfer owner window"))?;
            Ok(())
        })();
        let released = self
            .connection
            .ungrab_server()
            .map_err(|_| Failure::unavailable("cannot request text-transfer cleanup ungrab"))
            .and_then(|cookie| {
                cookie
                    .check()
                    .map_err(|_| Failure::unavailable("cannot release text-transfer cleanup grab"))
            });
        match (cleanup, released) {
            (Ok(()), Ok(())) => self
                .connection
                .sync()
                .map_err(|_| Failure::unavailable("cannot synchronize text-transfer cleanup")),
            (Err(error), _) | (Ok(()), Err(error)) => Err(error),
        }
    }
}

const fn transfer_reply(
    target: agent_seat_proto::TargetRequest,
    requested_bytes: u32,
    terminal: TextTransferTerminal,
) -> TextTransferReply {
    TextTransferReply {
        target,
        requested_bytes,
        delivered_bytes: 0,
        terminal,
    }
}
