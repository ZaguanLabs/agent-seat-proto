//! Provider-local, target-aware XTEST input behind the volatile seat gate.

use agent_seat_proto::{
    InputReply, InputTerminal, KeyboardTypeRequest, PointerButton, PointerClickRequest,
    PointerMoveRequest,
};
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::wrapper::ConnectionExt as _;

use super::{Failure, Observer};
use crate::seat::{SeatGate, SeatPermit};

const MAX_WINDOW_ANCESTORS: usize = 64;
const MAX_HIT_TEST_CHILDREN: usize = 256;
const MAX_HIT_TEST_RECTANGLES: usize = 256;
const POINTER_ROOT_FOCUS: u32 = 1;
const XK_TAB: u32 = 0xff09;
const XK_RETURN: u32 = 0xff0d;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_SHIFT_R: u32 = 0xffe2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeyStroke {
    keycode: u8,
    shift: bool,
}

impl Observer {
    pub(crate) fn pointer_move(
        &mut self,
        request: PointerMoveRequest,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<InputReply, Failure> {
        let sent = self.under_server_grab(|observer| {
            let (_, root_x, root_y) =
                observer.pointer_destination(request.target, request.x, request.y)?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            observer.fake_input(MOTION_NOTIFY_EVENT, 0, root_x, root_y)?;
            Ok(true)
        })?;
        Ok(action_reply(sent, seat.accepts(seat_permit)))
    }

    pub(crate) fn pointer_click(
        &mut self,
        request: PointerClickRequest,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<InputReply, Failure> {
        let sent = self.under_server_grab(|observer| {
            let (_, root_x, root_y) =
                observer.pointer_destination(request.target, request.x, request.y)?;
            if !seat.accepts(seat_permit) {
                return Ok(false);
            }
            observer.fake_input(MOTION_NOTIFY_EVENT, 0, root_x, root_y)?;
            observer.click_button(request.button, root_x, root_y)?;
            Ok(true)
        })?;
        Ok(action_reply(sent, seat.accepts(seat_permit)))
    }

    pub(crate) fn keyboard_type(
        &mut self,
        request: KeyboardTypeRequest,
        seat: &SeatGate,
        seat_permit: SeatPermit,
    ) -> Result<InputReply, Failure> {
        let requested = u16::try_from(request.text.chars().count())
            .map_err(|_| Failure::invalid("keyboard text exceeds the action bound"))?;
        let (strokes, shift_keycode) = self.under_server_grab(|observer| {
            observer.refresh()?;
            let target = observer.target(request.target)?;
            observer.require_focus_owned_by(target.xid)?;
            observer.resolve_text(request.text.as_str())
        })?;
        let mut completed = 0_u16;
        for stroke in strokes {
            let result = self.under_server_grab(|observer| {
                observer.refresh()?;
                let target = observer.target(request.target)?;
                observer.require_focus_owned_by(target.xid)?;
                if !seat.accepts(seat_permit) {
                    return Ok(false);
                }
                observer.type_key(stroke, shift_keycode)?;
                Ok(true)
            });
            match result {
                Ok(true) => {
                    completed = completed
                        .checked_add(1)
                        .ok_or_else(|| Failure::internal("keyboard action count overflowed"))?;
                }
                Ok(false) => break,
                Err(error) if completed == 0 => return Err(error),
                Err(_) => break,
            }
            if !seat.accepts(seat_permit) {
                break;
            }
        }
        let complete = completed == requested && seat.accepts(seat_permit);
        Ok(InputReply {
            completed,
            requested,
            terminal: if complete {
                InputTerminal::Queued
            } else {
                InputTerminal::Interrupted
            },
        })
    }

    fn under_server_grab<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        self.connection
            .grab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server grab"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot acquire an X11 server grab"))?;
        let result = action(self);
        let released = self
            .connection
            .ungrab_server()
            .map_err(|_| Failure::unavailable("cannot request an X11 server ungrab"))
            .and_then(|cookie| {
                cookie
                    .check()
                    .map_err(|_| Failure::unavailable("cannot release the X11 server grab"))
            });
        match (result, released) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    fn pointer_destination(
        &mut self,
        target_request: agent_seat_proto::TargetRequest,
        x: u32,
        y: u32,
    ) -> Result<(u32, i16, i16), Failure> {
        self.refresh()?;
        let target = self.target(target_request)?;
        let geometry = self
            .connection
            .get_geometry(target.xid)
            .map_err(|_| Failure::unavailable("cannot inspect target geometry"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect target geometry"))?;
        if x >= u32::from(geometry.width) || y >= u32::from(geometry.height) {
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
        let root_x = checked_root_coordinate(origin.dst_x, x)?;
        let root_y = checked_root_coordinate(origin.dst_y, y)?;
        self.require_point_owned_by(target.xid, root_x, root_y)?;
        Ok((target.xid, root_x, root_y))
    }

    fn click_button(&self, button: PointerButton, root_x: i16, root_y: i16) -> Result<(), Failure> {
        let detail = match button {
            PointerButton::Primary => 1,
            PointerButton::Middle => 2,
            PointerButton::Secondary => 3,
        };
        self.fake_input(BUTTON_PRESS_EVENT, detail, root_x, root_y)?;
        if let Err(error) = self.fake_input(BUTTON_RELEASE_EVENT, detail, root_x, root_y) {
            self.best_effort_release(BUTTON_RELEASE_EVENT, detail);
            return Err(error);
        }
        Ok(())
    }

    fn type_key(&self, stroke: KeyStroke, shift_keycode: Option<u8>) -> Result<(), Failure> {
        let shift =
            if stroke.shift {
                Some(shift_keycode.ok_or_else(|| {
                    Failure::unavailable("current X11 keyboard map has no Shift key")
                })?)
            } else {
                None
            };
        if let Some(shift) = shift {
            self.fake_input(KEY_PRESS_EVENT, shift, 0, 0)?;
        }
        if let Err(error) = self.fake_input(KEY_PRESS_EVENT, stroke.keycode, 0, 0) {
            if let Some(shift) = shift {
                self.best_effort_release(KEY_RELEASE_EVENT, shift);
            }
            return Err(error);
        }
        if let Err(error) = self.fake_input(KEY_RELEASE_EVENT, stroke.keycode, 0, 0) {
            self.best_effort_release(KEY_RELEASE_EVENT, stroke.keycode);
            if let Some(shift) = shift {
                self.best_effort_release(KEY_RELEASE_EVENT, shift);
            }
            return Err(error);
        }
        if let Some(shift) = shift {
            if let Err(error) = self.fake_input(KEY_RELEASE_EVENT, shift, 0, 0) {
                self.best_effort_release(KEY_RELEASE_EVENT, shift);
                return Err(error);
            }
        }
        Ok(())
    }

    fn fake_input(
        &self,
        event_type: u8,
        detail: u8,
        root_x: i16,
        root_y: i16,
    ) -> Result<(), Failure> {
        self.connection
            .xtest_fake_input(
                event_type,
                detail,
                CURRENT_TIME,
                self.root,
                root_x,
                root_y,
                0,
            )
            .map_err(|_| Failure::unavailable("cannot queue XTEST input"))?
            .check()
            .map_err(|_| Failure::unavailable("the X server refused XTEST input"))?;
        self.connection
            .sync()
            .map_err(|_| Failure::unavailable("cannot synchronize XTEST input"))
    }

    fn best_effort_release(&self, event_type: u8, detail: u8) {
        if let Ok(cookie) =
            self.connection
                .xtest_fake_input(event_type, detail, CURRENT_TIME, self.root, 0, 0, 0)
        {
            cookie.ignore_error();
            let _ = self.connection.sync();
        }
    }

    fn resolve_text(&self, text: &str) -> Result<(Vec<KeyStroke>, Option<u8>), Failure> {
        let setup = self.connection.setup();
        let count = setup
            .max_keycode
            .checked_sub(setup.min_keycode)
            .and_then(|difference| difference.checked_add(1))
            .ok_or_else(|| Failure::unavailable("X11 keyboard range is invalid"))?;
        let mapping = self
            .connection
            .get_keyboard_mapping(setup.min_keycode, count)
            .map_err(|_| Failure::unavailable("cannot inspect the X11 keyboard map"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect the X11 keyboard map"))?;
        let columns = usize::from(mapping.keysyms_per_keycode);
        if columns == 0 || mapping.keysyms.len() != usize::from(count) * columns {
            return Err(Failure::unavailable("X11 keyboard map is incomplete"));
        }
        let strokes = text
            .chars()
            .map(|character| {
                let keysym = keysym_for_character(character);
                find_stroke(&mapping.keysyms, columns, setup.min_keycode, keysym).ok_or_else(|| {
                    Failure::invalid(
                        "text contains a character unavailable in the current X11 keyboard layout",
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let shift_keycode = strokes
            .iter()
            .any(|stroke| stroke.shift)
            .then(|| {
                find_keycode(
                    &mapping.keysyms,
                    columns,
                    setup.min_keycode,
                    &[XK_SHIFT_L, XK_SHIFT_R],
                )
                .ok_or_else(|| Failure::unavailable("current X11 keyboard map has no Shift key"))
            })
            .transpose()?;
        Ok((strokes, shift_keycode))
    }

    fn require_focus_owned_by(&self, target: u32) -> Result<(), Failure> {
        let focus = self
            .connection
            .get_input_focus()
            .map_err(|_| Failure::unavailable("cannot inspect X11 input focus"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect X11 input focus"))?
            .focus;
        if focus == x11rb::NONE || focus == POINTER_ROOT_FOCUS {
            return Err(Failure::invalid(
                "keyboard focus is not owned by the target client",
            ));
        }
        let mut window = focus;
        for _ in 0..MAX_WINDOW_ANCESTORS {
            if window == target {
                return Ok(());
            }
            let parent = self
                .connection
                .query_tree(window)
                .map_err(|_| Failure::unavailable("cannot inspect keyboard focus ancestry"))?
                .reply()
                .map_err(|_| Failure::unavailable("cannot inspect keyboard focus ancestry"))?
                .parent;
            if parent == x11rb::NONE || parent == self.root {
                break;
            }
            window = parent;
        }
        Err(Failure::invalid(
            "keyboard focus is not owned by the target client",
        ))
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

fn action_reply(sent: bool, seat_still_enabled: bool) -> InputReply {
    InputReply {
        completed: u16::from(sent),
        requested: 1,
        terminal: if sent && seat_still_enabled {
            InputTerminal::Queued
        } else {
            InputTerminal::Interrupted
        },
    }
}

fn checked_root_coordinate(origin: i16, offset: u32) -> Result<i16, Failure> {
    i32::from(origin)
        .checked_add(
            i32::try_from(offset).map_err(|_| {
                Failure::invalid("pointer destination exceeds X11 coordinate range")
            })?,
        )
        .and_then(|value| i16::try_from(value).ok())
        .ok_or_else(|| Failure::invalid("pointer destination exceeds X11 coordinate range"))
}

fn keysym_for_character(character: char) -> u32 {
    match character {
        '\t' => XK_TAB,
        '\n' => XK_RETURN,
        value if u32::from(value) <= 0xff => u32::from(value),
        value => 0x0100_0000 | u32::from(value),
    }
}

fn find_stroke(keysyms: &[u32], columns: usize, minimum: u8, wanted: u32) -> Option<KeyStroke> {
    if columns == 0 {
        return None;
    }
    keysyms
        .chunks_exact(columns)
        .enumerate()
        .find_map(|(row, symbols)| {
            symbols
                .iter()
                .take(2)
                .enumerate()
                .find_map(|(level, symbol)| {
                    (*symbol == wanted).then(|| {
                        u8::try_from(row)
                            .ok()
                            .and_then(|row| minimum.checked_add(row))
                            .map(|keycode| KeyStroke {
                                keycode,
                                shift: level == 1,
                            })
                    })?
                })
        })
}

fn find_keycode(keysyms: &[u32], columns: usize, minimum: u8, wanted: &[u32]) -> Option<u8> {
    if columns == 0 {
        return None;
    }
    keysyms
        .chunks_exact(columns)
        .enumerate()
        .find_map(|(row, symbols)| {
            symbols
                .iter()
                .any(|symbol| wanted.contains(symbol))
                .then(|| {
                    u8::try_from(row)
                        .ok()
                        .and_then(|row| minimum.checked_add(row))
                })?
        })
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

    #[test]
    fn first_group_keysyms_resolve_without_backend_keycodes_on_the_wire() {
        let keysyms = [
            u32::from('a'),
            u32::from('A'),
            u32::from('1'),
            u32::from('!'),
        ];
        assert_eq!(
            find_stroke(&keysyms, 2, 8, u32::from('a')),
            Some(KeyStroke {
                keycode: 8,
                shift: false,
            })
        );
        assert_eq!(
            find_stroke(&keysyms, 2, 8, u32::from('!')),
            Some(KeyStroke {
                keycode: 9,
                shift: true,
            })
        );
        assert_eq!(find_stroke(&keysyms, 2, 8, u32::from('z')), None);
    }
}
