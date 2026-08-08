//! Atomic per-screen X11 selection and advertisement ownership.

use std::path::Path;

use agent_seat_proto::{ADVERTISEMENT_PROPERTY, Advertisement};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, PropMode, WindowClass};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

pub(crate) struct Ownership {
    connection: RustConnection,
    screen: usize,
    root: u32,
    owner: u32,
    selection: u32,
    property: u32,
    active: bool,
}

impl Ownership {
    pub(crate) fn claim(socket: &Path) -> Result<Self, String> {
        let socket = socket
            .to_str()
            .ok_or_else(|| "socket path must be UTF-8".to_owned())?;
        let advertisement = Advertisement::new(socket)
            .map_err(|error| format!("cannot advertise provider: {error}"))?
            .encode();
        let (connection, screen) = x11rb::connect(None)
            .map_err(|error| format!("cannot connect to selected X11 display: {error}"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| "selected X11 screen is absent".to_owned())?
            .root;
        let selection = intern(&connection, format!("_AGENT_SEAT_S{screen}").as_bytes())?;
        let property = intern(&connection, ADVERTISEMENT_PROPERTY.as_bytes())?;
        let utf8 = intern(&connection, b"UTF8_STRING")?;
        let owner = connection
            .generate_id()
            .map_err(|error| format!("cannot allocate X11 owner window: {error}"))?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                owner,
                root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;

        if let Err(error) = claim_selection(&connection, selection, owner) {
            let _ = connection.destroy_window(owner);
            let _ = connection.flush();
            return Err(error);
        }
        for window in [owner, root] {
            if let Err(error) = connection
                .change_property8(
                    PropMode::REPLACE,
                    window,
                    property,
                    utf8,
                    advertisement.as_bytes(),
                )
                .map_err(x11_error)
                .and_then(|cookie| cookie.check().map_err(x11_error))
            {
                let _ = release(&connection, root, owner, selection, property);
                return Err(error);
            }
        }
        connection.flush().map_err(x11_error)?;
        Ok(Self {
            connection,
            screen,
            root,
            owner,
            selection,
            property,
            active: true,
        })
    }

    pub(crate) const fn screen(&self) -> usize {
        self.screen
    }

    pub(crate) fn lost(&self) -> Result<bool, String> {
        while let Some(event) = self.connection.poll_for_event().map_err(x11_error)? {
            if matches!(
                event,
                Event::SelectionClear(event)
                    if event.selection == self.selection && event.owner == self.owner
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn withdraw(&mut self) -> Result<(), String> {
        let result = release(
            &self.connection,
            self.root,
            self.owner,
            self.selection,
            self.property,
        );
        self.active = false;
        result
    }
}

pub(crate) fn selected_screen() -> Result<usize, String> {
    x11rb::connect(None)
        .map(|(_, screen)| screen)
        .map_err(|error| format!("cannot connect to selected X11 display: {error}"))
}

impl Drop for Ownership {
    fn drop(&mut self) {
        if self.active {
            let _ = release(
                &self.connection,
                self.root,
                self.owner,
                self.selection,
                self.property,
            );
        }
    }
}

fn claim_selection(connection: &RustConnection, selection: u32, owner: u32) -> Result<(), String> {
    connection
        .grab_server()
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    let claim = (|| {
        let current = connection
            .get_selection_owner(selection)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?
            .owner;
        if current != NONE {
            return Err("another Agent Seat provider already owns this X11 screen".to_owned());
        }
        connection
            .set_selection_owner(owner, selection, CURRENT_TIME)
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;
        let current = connection
            .get_selection_owner(selection)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?
            .owner;
        if current != owner {
            return Err("Agent Seat selection ownership could not be established".to_owned());
        }
        Ok(())
    })();
    let ungrab = connection
        .ungrab_server()
        .map_err(x11_error)
        .and_then(|cookie| cookie.check().map_err(x11_error));
    let flush = connection.flush().map_err(x11_error);
    claim.and(ungrab).and(flush)
}

fn release(
    connection: &RustConnection,
    root: u32,
    owner: u32,
    selection: u32,
    property: u32,
) -> Result<(), String> {
    connection
        .grab_server()
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    let withdrawal = (|| {
        let current = connection
            .get_selection_owner(selection)
            .map_err(x11_error)?
            .reply()
            .map_err(x11_error)?
            .owner;
        if current != owner {
            return Err("Agent Seat selection was lost before withdrawal".to_owned());
        }
        connection
            .delete_property(root, property)
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;
        connection
            .delete_property(owner, property)
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;
        connection
            .set_selection_owner(NONE, selection, CURRENT_TIME)
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)
    })();
    let destroy = connection
        .destroy_window(owner)
        .map_err(x11_error)
        .and_then(|cookie| cookie.check().map_err(x11_error));
    let ungrab = connection
        .ungrab_server()
        .map_err(x11_error)
        .and_then(|cookie| cookie.check().map_err(x11_error));
    let flush = connection.flush().map_err(x11_error);
    withdrawal.and(destroy).and(ungrab).and(flush)
}

fn intern(connection: &RustConnection, name: &[u8]) -> Result<u32, String> {
    connection
        .intern_atom(false, name)
        .map_err(x11_error)?
        .reply()
        .map(|reply| reply.atom)
        .map_err(x11_error)
}

fn x11_error(error: impl std::fmt::Display) -> String {
    format!("X11 provider ownership failed: {error}")
}
