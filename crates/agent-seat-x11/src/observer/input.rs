//! One-action XTEST gate backed by an independent physical-activity broker.

use std::path::Path;
use std::time::Duration;

use agent_seat_activity_broker::{BrokerConnection, BrokerState};
use agent_seat_proto::{InputReply, InputTerminal, PointerMoveRequest};
use x11rb::CURRENT_TIME;
use x11rb::protocol::xproto::{ConnectionExt as _, MOTION_NOTIFY_EVENT};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;

use super::{Failure, Observer};

const MAX_WINDOW_ANCESTORS: usize = 64;

impl Observer {
    pub(crate) fn pointer_move(
        &mut self,
        request: PointerMoveRequest,
        broker_socket: &Path,
        broker_peer_uid: u32,
        timeout: Duration,
    ) -> Result<InputReply, Failure> {
        let mut broker = BrokerConnection::connect(broker_socket, timeout, broker_peer_uid)
            .map_err(|_| Failure::unavailable("cannot connect to the activity broker"))?;
        let initial = broker
            .status()
            .map_err(|_| Failure::unavailable("cannot read activity broker state"))?;
        if !matches!(initial.state, BrokerState::Ready) {
            return Err(Failure::unavailable("physical activity gate is not ready"));
        }

        self.connection
            .grab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server grab"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot acquire an X11 server grab"))?;

        let sent = self.prepare_and_move(request, &mut broker, initial);
        let released = self
            .connection
            .ungrab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server ungrab"))
            .and_then(|cookie| {
                cookie
                    .check()
                    .map_err(|_| Failure::unavailable("cannot release the X11 server grab"))
            });
        let sent = sent?;
        released?;

        let terminal = match (sent, broker.status()) {
            (true, Ok(status)) if initial.is_same_ready(status) => InputTerminal::Queued,
            _ => InputTerminal::Interrupted,
        };
        Ok(InputReply {
            completed: u16::from(sent),
            requested: 1,
            terminal,
        })
    }

    fn prepare_and_move(
        &mut self,
        request: PointerMoveRequest,
        broker: &mut BrokerConnection,
        initial: agent_seat_activity_broker::BrokerStatus,
    ) -> Result<bool, Failure> {
        self.refresh()?;
        let target = self.target(request.target)?;
        let geometry = self
            .connection
            .get_geometry(target.xid)
            .map_err(|_| Failure::unavailable("cannot inspect target geometry"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect target geometry"))?;
        if request.x >= u32::from(geometry.width) || request.y >= u32::from(geometry.height) {
            return Err(Failure::invalid(
                "pointer destination is outside the target client",
            ));
        }
        let origin = self
            .connection
            .translate_coordinates(target.xid, self.root, 0, 0)
            .map_err(|_| Failure::unavailable("cannot translate target coordinates"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot translate target coordinates"))?;
        let root_x = i32::from(origin.dst_x)
            .checked_add(i32::try_from(request.x).map_err(|_| {
                Failure::invalid("pointer destination exceeds X11 coordinate range")
            })?)
            .and_then(|value| i16::try_from(value).ok())
            .ok_or_else(|| Failure::invalid("pointer destination exceeds X11 coordinate range"))?;
        let root_y = i32::from(origin.dst_y)
            .checked_add(i32::try_from(request.y).map_err(|_| {
                Failure::invalid("pointer destination exceeds X11 coordinate range")
            })?)
            .and_then(|value| i16::try_from(value).ok())
            .ok_or_else(|| Failure::invalid("pointer destination exceeds X11 coordinate range"))?;

        self.require_point_owned_by(target.xid, root_x, root_y)?;
        let under_grab = broker
            .status()
            .map_err(|_| Failure::unavailable("activity broker evidence was lost"))?;
        if !initial.is_same_ready(under_grab) {
            return Ok(false);
        }

        self.connection
            .xtest_fake_input(
                MOTION_NOTIFY_EVENT,
                0,
                CURRENT_TIME,
                self.root,
                root_x,
                root_y,
                0,
            )
            .map_err(|_| Failure::unavailable("cannot queue pointer movement"))?
            .check()
            .map_err(|_| Failure::unavailable("the X server refused pointer movement"))?;
        self.connection
            .sync()
            .map_err(|_| Failure::unavailable("cannot synchronize pointer movement"))?;
        Ok(true)
    }

    fn require_point_owned_by(&self, target: u32, root_x: i16, root_y: i16) -> Result<(), Failure> {
        let mut window = self.root;
        for _ in 0..MAX_WINDOW_ANCESTORS {
            if window == target {
                return Ok(());
            }
            let child = self
                .connection
                .translate_coordinates(self.root, window, root_x, root_y)
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
                .child;
            if child == x11rb::NONE {
                break;
            }
            window = child;
        }
        Err(Failure::invalid(
            "pointer destination is not visibly owned by the target",
        ))
    }
}
