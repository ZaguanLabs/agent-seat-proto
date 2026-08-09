//! One-action XTEST gate backed by an independent physical-activity broker.

use std::path::Path;
use std::time::Duration;

use agent_seat_activity_broker::{BrokerConnection, BrokerState};
use agent_seat_proto::{InputReply, InputTerminal, PointerMoveRequest};
use x11rb::CURRENT_TIME;
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xproto::{ConnectionExt as _, MOTION_NOTIFY_EVENT};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;

use super::{Failure, Observer};

const MAX_WINDOW_ANCESTORS: usize = 64;
const MAX_HIT_TEST_CHILDREN: usize = 256;
const MAX_HIT_TEST_RECTANGLES: usize = 256;

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
        let children = self
            .connection
            .query_tree(self.root)
            .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
            .children;
        if children.len() > MAX_HIT_TEST_CHILDREN {
            return Err(Failure::unavailable(
                "pointer hit-test exceeds the window bound",
            ));
        }
        let mut destination = None;
        // QueryTree returns siblings from bottom to top; inspect the effective
        // input shapes in reverse so the first match is the actual top level.
        for child in children.into_iter().rev() {
            let attributes = self
                .connection
                .get_window_attributes(child)
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?;
            if attributes.map_state != x11rb::protocol::xproto::MapState::VIEWABLE {
                continue;
            }
            let translated = self
                .connection
                .translate_coordinates(self.root, child, root_x, root_y)
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot hit-test pointer destination"))?;
            let input = self
                .connection
                .shape_get_rectangles(child, SK::INPUT)
                .map_err(|_| Failure::unavailable("cannot inspect pointer input shape"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot inspect pointer input shape"))?;
            if input.rectangles.len() > MAX_HIT_TEST_RECTANGLES {
                return Err(Failure::unavailable(
                    "pointer hit-test exceeds the shape bound",
                ));
            }
            if input
                .rectangles
                .iter()
                .any(|rectangle| rectangle_contains(rectangle, translated.dst_x, translated.dst_y))
            {
                destination = Some(child);
                break;
            }
        }
        if let Some(destination) = destination {
            if self.is_target_or_reparenting_frame(destination, target)? {
                return Ok(());
            }
        }
        Err(Failure::invalid(
            "pointer destination is not visibly owned by the target",
        ))
    }

    fn is_target_or_reparenting_frame(&self, candidate: u32, target: u32) -> Result<bool, Failure> {
        let mut window = target;
        for _ in 0..MAX_WINDOW_ANCESTORS {
            if window == candidate {
                return Ok(true);
            }
            let parent = self
                .connection
                .query_tree(window)
                .map_err(|_| Failure::unavailable("cannot inspect target ancestry"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot inspect target ancestry"))?
                .parent;
            if parent == x11rb::NONE || parent == self.root {
                return Ok(parent == candidate);
            }
            window = parent;
        }
        Err(Failure::unavailable(
            "target ancestry exceeds the window bound",
        ))
    }
}

fn rectangle_contains(rectangle: &x11rb::protocol::xproto::Rectangle, x: i16, y: i16) -> bool {
    let x = i32::from(x);
    let y = i32::from(y);
    let left = i32::from(rectangle.x);
    let top = i32::from(rectangle.y);
    x >= left
        && y >= top
        && x < left + i32::from(rectangle.width)
        && y < top + i32::from(rectangle.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_rectangles_use_half_open_bounds() {
        let rectangle = x11rb::protocol::xproto::Rectangle {
            x: -2,
            y: 3,
            width: 5,
            height: 7,
        };
        assert!(rectangle_contains(&rectangle, -2, 3));
        assert!(rectangle_contains(&rectangle, 2, 9));
        assert!(!rectangle_contains(&rectangle, 3, 9));
        assert!(!rectangle_contains(&rectangle, 2, 10));
    }
}
