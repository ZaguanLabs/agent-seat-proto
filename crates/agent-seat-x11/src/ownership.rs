//! Atomic per-screen X11 selection and advertisement ownership.

use std::path::{Path, PathBuf};

use agent_seat_proto::{ADVERTISEMENT_PROPERTY, Advertisement, MAX_ADVERTISEMENT_BYTES};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, PropMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

const MAX_PROPERTY_LONGS: u32 = (MAX_ADVERTISEMENT_BYTES as u32).div_ceil(4) + 1;

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

pub(crate) fn advertised_socket() -> Result<PathBuf, String> {
    let (connection, screen_index) = x11rb::connect(None)
        .map_err(|error| format!("cannot inspect X11 Agent Seat discovery: {error}"))?;
    let screen = connection.setup().roots.get(screen_index).ok_or_else(|| {
        "cannot inspect X11 Agent Seat discovery: selected screen is absent".to_owned()
    })?;
    let selection = lookup(
        &connection,
        format!("_AGENT_SEAT_S{screen_index}").as_bytes(),
    )?;
    if selection == NONE {
        return Err("no Agent Seat provider is advertised on the selected X11 screen".to_owned());
    }
    let owner = connection
        .get_selection_owner(selection)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?
        .owner;
    if owner == NONE {
        return Err("no Agent Seat provider is advertised on the selected X11 screen".to_owned());
    }
    let property = lookup(&connection, ADVERTISEMENT_PROPERTY.as_bytes())?;
    let utf8 = lookup(&connection, b"UTF8_STRING")?;
    if property == NONE || utf8 == NONE {
        return Err("the selected X11 screen has no Agent Seat advertisement atoms".to_owned());
    }
    let owner_value = read_advertisement(&connection, owner, property, utf8)?;
    let root_value = read_advertisement(&connection, screen.root, property, utf8)?;
    if owner_value != root_value {
        return Err("the X11 Agent Seat advertisements do not match".to_owned());
    }
    let current_owner = connection
        .get_selection_owner(selection)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?
        .owner;
    if current_owner != owner {
        return Err("the X11 Agent Seat provider changed during discovery".to_owned());
    }
    let encoded = std::str::from_utf8(&root_value)
        .map_err(|_| "the X11 Agent Seat advertisement is not UTF-8".to_owned())?;
    let advertisement = Advertisement::parse(encoded)
        .map_err(|error| format!("invalid X11 Agent Seat advertisement: {error}"))?;
    Ok(PathBuf::from(advertisement.socket()))
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

fn lookup(connection: &RustConnection, name: &[u8]) -> Result<u32, String> {
    connection
        .intern_atom(true, name)
        .map_err(x11_error)?
        .reply()
        .map(|reply| reply.atom)
        .map_err(x11_error)
}

fn read_advertisement(
    connection: &RustConnection,
    window: u32,
    property: u32,
    utf8: u32,
) -> Result<Vec<u8>, String> {
    let reply = connection
        .get_property(
            false,
            window,
            property,
            AtomEnum::ANY,
            0,
            MAX_PROPERTY_LONGS,
        )
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?;
    if reply.type_ != utf8
        || reply.format != 8
        || reply.bytes_after != 0
        || reply.value.len() > MAX_ADVERTISEMENT_BYTES
    {
        return Err(
            "the X11 Agent Seat advertisement has invalid type, format, or size".to_owned(),
        );
    }
    Ok(reply.value)
}

fn x11_error(error: impl std::fmt::Display) -> String {
    format!("X11 provider ownership failed: {error}")
}
