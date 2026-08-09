//! XKB-aware resolution from Unicode scalars to physical key actions.

use agent_seat_proto::{KeyboardKey, KeyboardModifier};
use x11rb::protocol::xkb::{
    ConnectionExt as _, GetMapReply, GetStateReply, ID, KeyModMap, KeySymMap, KeyType, MapPart,
    VMod,
};
use x11rb::protocol::xproto::ModMask;
use x11rb::rust_connection::RustConnection;

use super::Failure;

const XK_TAB: u32 = 0xff09;
const XK_RETURN: u32 = 0xff0d;
const XK_BACKSPACE: u32 = 0xff08;
const XK_ESCAPE: u32 = 0xff1b;
const XK_DELETE: u32 = 0xffff;
const XK_HOME: u32 = 0xff50;
const XK_LEFT: u32 = 0xff51;
const XK_UP: u32 = 0xff52;
const XK_RIGHT: u32 = 0xff53;
const XK_DOWN: u32 = 0xff54;
const XK_PRIOR: u32 = 0xff55;
const XK_NEXT: u32 = 0xff56;
const XK_END: u32 = 0xff57;
const XK_INSERT: u32 = 0xff63;
const XK_F1: u32 = 0xffbe;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_SHIFT_R: u32 = 0xffe2;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_CONTROL_R: u32 = 0xffe4;
const XK_CAPS_LOCK: u32 = 0xffe5;
const XK_SHIFT_LOCK: u32 = 0xffe6;
const XK_NUM_LOCK: u32 = 0xff7f;
const XK_ALT_L: u32 = 0xffe9;
const XK_ALT_R: u32 = 0xffea;
const XK_SUPER_L: u32 = 0xffeb;
const XK_SUPER_R: u32 = 0xffec;
const XK_ISO_LEVEL3_SHIFT: u32 = 0xfe03;
const XK_ISO_LEVEL5_SHIFT: u32 = 0xfe11;
const MAX_MOMENTARY_MODIFIERS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct KeyStroke {
    pub(super) keycode: u8,
    pub(super) modifiers: Vec<u8>,
}

struct KeyboardMap {
    minimum: u8,
    types: Vec<KeyType>,
    symbols: Vec<KeySymMap>,
    modifier_map: Vec<KeyModMap>,
    state: GetStateReply,
}

impl KeyboardMap {
    fn read(connection: &RustConnection) -> Result<Self, Failure> {
        let extension = connection
            .xkb_use_extension(1, 0)
            .map_err(|_| Failure::unavailable("cannot negotiate the XKB extension"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot negotiate the XKB extension"))?;
        if !extension.supported {
            return Err(Failure::unavailable(
                "the X server does not support the required XKB extension",
            ));
        }

        let device = u16::from(ID::USE_CORE_KBD);
        let state = connection
            .xkb_get_state(device)
            .map_err(|_| Failure::unavailable("cannot inspect the XKB keyboard state"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect the XKB keyboard state"))?;
        let parts = MapPart::KEY_TYPES | MapPart::KEY_SYMS | MapPart::MODIFIER_MAP;
        let reply = connection
            .xkb_get_map(
                device,
                parts,
                MapPart::default(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                VMod::default(),
                0,
                0,
                0,
                0,
                0,
                0,
            )
            .map_err(|_| Failure::unavailable("cannot inspect the XKB keyboard map"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect the XKB keyboard map"))?;
        Self::from_reply(reply, state)
    }

    fn from_reply(reply: GetMapReply, state: GetStateReply) -> Result<Self, Failure> {
        let types = reply
            .map
            .types_rtrn
            .ok_or_else(|| Failure::unavailable("XKB returned no key types"))?;
        let symbols = reply
            .map
            .syms_rtrn
            .ok_or_else(|| Failure::unavailable("XKB returned no key symbols"))?;
        let modifier_map = reply
            .map
            .modmap_rtrn
            .ok_or_else(|| Failure::unavailable("XKB returned no modifier map"))?;
        let expected_symbols = reply
            .max_key_code
            .checked_sub(reply.min_key_code)
            .and_then(|difference| difference.checked_add(1))
            .map(usize::from);
        if reply.first_type != 0
            || reply.n_types != reply.total_types
            || types.len() != usize::from(reply.n_types)
            || reply.first_key_sym != reply.min_key_code
            || expected_symbols != Some(symbols.len())
            || symbols.len() != usize::from(reply.n_key_syms)
        {
            return Err(Failure::unavailable("XKB keyboard map is incomplete"));
        }
        Ok(Self {
            minimum: reply.first_key_sym,
            types,
            symbols,
            modifier_map,
            state,
        })
    }

    fn resolve(&self, wanted: u32) -> Result<KeyStroke, Failure> {
        self.resolve_with_modifiers(wanted, &[])
    }

    fn resolve_with_modifiers(
        &self,
        wanted: u32,
        requested: &[KeyboardModifier],
    ) -> Result<KeyStroke, Failure> {
        if u16::from(self.state.base_mods) != 0
            || u16::from(self.state.latched_mods) != 0
            || self.state.base_group != 0
            || self.state.latched_group != 0
        {
            return Err(Failure::unavailable(
                "keyboard input is unavailable while a depressed or latched modifier is active",
            ));
        }
        let group = usize::from(u8::from(self.state.group));
        let mut requested_bits = 0_u8;
        let mut requested_keys = Vec::with_capacity(requested.len());
        for modifier in requested {
            let key = self.modifier_key(*modifier)?;
            if requested_bits & key.modifiers != 0 {
                return Err(Failure::invalid(
                    "requested keyboard modifiers share an XKB modifier bit",
                ));
            }
            requested_bits |= key.modifiers;
            requested_keys.push(key.keycode);
        }
        let current_modifiers = modifier_bits(self.state.mods)? | requested_bits;
        let safe_locked_modifiers = self.safe_locked_modifiers()?;
        let modifier_keys = self.momentary_modifier_keys();

        self.symbols
            .iter()
            .enumerate()
            .filter_map(|(row, key)| {
                let keycode = u8::try_from(row)
                    .ok()
                    .and_then(|row| self.minimum.checked_add(row))?;
                let group = normalized_group(key.group_info, group)?;
                let key_type = self.types.get(usize::from(*key.kt_index.get(group)?))?;
                let type_modifiers = modifier_bits(key_type.mods_mask).ok()?;
                if current_modifiers & !(type_modifiers | safe_locked_modifiers | requested_bits)
                    != 0
                {
                    return None;
                }
                let width = usize::from(key.width);
                let start = group.checked_mul(width)?;
                let symbols = key.syms.get(start..start.checked_add(width)?)?;
                symbols
                    .iter()
                    .enumerate()
                    .take(usize::from(key_type.num_levels))
                    .filter(|(_, symbol)| **symbol == wanted)
                    .filter_map(|(level, _)| {
                        self.modifiers_for_level(
                            key_type,
                            u8::try_from(level).ok()?,
                            current_modifiers,
                            &modifier_keys,
                        )
                        .map(|modifiers| {
                            let mut all_modifiers = requested_keys.clone();
                            for keycode in modifiers {
                                if !all_modifiers.contains(&keycode) {
                                    all_modifiers.push(keycode);
                                }
                            }
                            KeyStroke {
                                keycode,
                                modifiers: all_modifiers,
                            }
                        })
                    })
                    .min_by_key(|stroke| stroke.modifiers.len())
            })
            .min_by_key(|stroke| stroke.modifiers.len())
            .ok_or_else(|| {
                Failure::invalid(
                    "keyboard symbol is unavailable in the current XKB layout and group",
                )
            })
    }

    fn modifier_key(&self, modifier: KeyboardModifier) -> Result<ModifierKey, Failure> {
        let symbols: &[u32] = match modifier {
            KeyboardModifier::Control => &[XK_CONTROL_L, XK_CONTROL_R],
            KeyboardModifier::Alt => &[XK_ALT_L, XK_ALT_R],
            KeyboardModifier::Shift => &[XK_SHIFT_L, XK_SHIFT_R],
            KeyboardModifier::Super => &[XK_SUPER_L, XK_SUPER_R],
        };
        self.modifier_map
            .iter()
            .find_map(|entry| {
                let bits = modifier_bits(entry.mods).ok()?;
                (bits.count_ones() == 1 && self.key_has_any_symbol(entry.keycode, symbols))
                    .then_some(ModifierKey {
                        keycode: entry.keycode,
                        modifiers: bits,
                    })
            })
            .ok_or_else(|| {
                Failure::invalid(
                    "keyboard modifier is unavailable in the current XKB layout and group",
                )
            })
    }

    fn momentary_modifier_keys(&self) -> Vec<ModifierKey> {
        let mut keys = Vec::new();
        for entry in &self.modifier_map {
            let Ok(modifiers) = modifier_bits(entry.mods) else {
                continue;
            };
            if modifiers.count_ones() != 1 || !self.key_has_momentary_modifier_symbol(entry.keycode)
            {
                continue;
            }
            if keys
                .iter()
                .any(|candidate: &ModifierKey| candidate.modifiers == modifiers)
            {
                continue;
            }
            if keys.len() == MAX_MOMENTARY_MODIFIERS {
                break;
            }
            keys.push(ModifierKey {
                keycode: entry.keycode,
                modifiers,
            });
        }
        keys
    }

    fn key_has_momentary_modifier_symbol(&self, keycode: u8) -> bool {
        let Some(row) = keycode.checked_sub(self.minimum).map(usize::from) else {
            return false;
        };
        self.symbols.get(row).is_some_and(|key| {
            key.syms.iter().any(|symbol| {
                matches!(
                    *symbol,
                    XK_SHIFT_L | XK_SHIFT_R | XK_ISO_LEVEL3_SHIFT | XK_ISO_LEVEL5_SHIFT
                )
            })
        })
    }

    fn safe_locked_modifiers(&self) -> Result<u8, Failure> {
        let locked = modifier_bits(self.state.locked_mods)?;
        let mut safe = 0_u8;
        for entry in &self.modifier_map {
            let modifiers = modifier_bits(entry.mods)?;
            if self.key_has_any_symbol(entry.keycode, &[XK_CAPS_LOCK, XK_SHIFT_LOCK, XK_NUM_LOCK]) {
                safe |= modifiers;
            }
        }
        Ok(locked & safe)
    }

    fn key_has_any_symbol(&self, keycode: u8, wanted: &[u32]) -> bool {
        let Some(row) = keycode.checked_sub(self.minimum).map(usize::from) else {
            return false;
        };
        self.symbols
            .get(row)
            .is_some_and(|key| key.syms.iter().any(|symbol| wanted.contains(symbol)))
    }

    fn modifiers_for_level(
        &self,
        key_type: &KeyType,
        wanted_level: u8,
        current: u8,
        keys: &[ModifierKey],
    ) -> Option<Vec<u8>> {
        let combinations = 1_u16.checked_shl(u32::try_from(keys.len()).ok()?)?;
        (0..combinations)
            .filter_map(|combination| {
                let mut effective = current;
                let mut keycodes = Vec::new();
                for (index, key) in keys.iter().enumerate() {
                    if combination & (1_u16 << index) != 0 {
                        effective |= key.modifiers;
                        keycodes.push(key.keycode);
                    }
                }
                (level_for_modifiers(key_type, effective) == wanted_level)
                    .then_some((keycodes, effective))
            })
            .min_by_key(|(keycodes, effective)| (keycodes.len(), effective.count_ones()))
            .map(|(keycodes, _)| keycodes)
    }
}

#[derive(Clone, Copy)]
struct ModifierKey {
    keycode: u8,
    modifiers: u8,
}

pub(super) fn resolve_text(
    connection: &RustConnection,
    text: &str,
) -> Result<Vec<KeyStroke>, Failure> {
    let map = KeyboardMap::read(connection)?;
    text.chars()
        .map(|character| map.resolve(keysym_for_character(character)))
        .collect()
}

pub(super) fn resolve_character(
    connection: &RustConnection,
    character: char,
) -> Result<KeyStroke, Failure> {
    KeyboardMap::read(connection)?.resolve(keysym_for_character(character))
}

pub(super) fn resolve_key(
    connection: &RustConnection,
    key: KeyboardKey,
    modifiers: &[KeyboardModifier],
) -> Result<KeyStroke, Failure> {
    let map = KeyboardMap::read(connection)?;
    map.resolve_with_modifiers(keysym_for_key(key), modifiers)
}

fn modifier_bits(mask: ModMask) -> Result<u8, Failure> {
    u8::try_from(u16::from(mask))
        .map_err(|_| Failure::unavailable("XKB returned an invalid modifier mask"))
}

fn normalized_group(group_info: u8, effective: usize) -> Option<usize> {
    let count = usize::from(group_info & 0x0f);
    if count == 0 {
        return None;
    }
    if effective < count {
        return Some(effective);
    }
    match group_info & 0xc0 {
        0x00 => Some(effective % count),
        0x40 => Some(count - 1),
        0x80 => {
            let redirected = usize::from((group_info & 0x30) >> 4);
            (redirected < count).then_some(redirected)
        }
        _ => None,
    }
}

fn level_for_modifiers(key_type: &KeyType, modifiers: u8) -> u8 {
    let relevant = modifiers & u8::try_from(u16::from(key_type.mods_mask)).unwrap_or(u8::MAX);
    key_type
        .map
        .iter()
        .find(|entry| {
            entry.active
                && u8::try_from(u16::from(entry.mods_mask)).is_ok_and(|mask| relevant == mask)
        })
        .map_or(0, |entry| entry.level)
}

fn keysym_for_character(character: char) -> u32 {
    match character {
        '\t' => XK_TAB,
        '\n' => XK_RETURN,
        value if u32::from(value) <= 0xff => u32::from(value),
        value => 0x0100_0000 | u32::from(value),
    }
}

fn keysym_for_key(key: KeyboardKey) -> u32 {
    match key {
        KeyboardKey::Backspace => XK_BACKSPACE,
        KeyboardKey::Delete => XK_DELETE,
        KeyboardKey::Enter => XK_RETURN,
        KeyboardKey::Escape => XK_ESCAPE,
        KeyboardKey::Tab => XK_TAB,
        KeyboardKey::Space => u32::from(b' '),
        KeyboardKey::Insert => XK_INSERT,
        KeyboardKey::Home => XK_HOME,
        KeyboardKey::End => XK_END,
        KeyboardKey::PageUp => XK_PRIOR,
        KeyboardKey::PageDown => XK_NEXT,
        KeyboardKey::ArrowLeft => XK_LEFT,
        KeyboardKey::ArrowRight => XK_RIGHT,
        KeyboardKey::ArrowUp => XK_UP,
        KeyboardKey::ArrowDown => XK_DOWN,
        KeyboardKey::A => u32::from(b'a'),
        KeyboardKey::B => u32::from(b'b'),
        KeyboardKey::C => u32::from(b'c'),
        KeyboardKey::D => u32::from(b'd'),
        KeyboardKey::E => u32::from(b'e'),
        KeyboardKey::F => u32::from(b'f'),
        KeyboardKey::G => u32::from(b'g'),
        KeyboardKey::H => u32::from(b'h'),
        KeyboardKey::I => u32::from(b'i'),
        KeyboardKey::J => u32::from(b'j'),
        KeyboardKey::K => u32::from(b'k'),
        KeyboardKey::L => u32::from(b'l'),
        KeyboardKey::M => u32::from(b'm'),
        KeyboardKey::N => u32::from(b'n'),
        KeyboardKey::O => u32::from(b'o'),
        KeyboardKey::P => u32::from(b'p'),
        KeyboardKey::Q => u32::from(b'q'),
        KeyboardKey::R => u32::from(b'r'),
        KeyboardKey::S => u32::from(b's'),
        KeyboardKey::T => u32::from(b't'),
        KeyboardKey::U => u32::from(b'u'),
        KeyboardKey::V => u32::from(b'v'),
        KeyboardKey::W => u32::from(b'w'),
        KeyboardKey::X => u32::from(b'x'),
        KeyboardKey::Y => u32::from(b'y'),
        KeyboardKey::Z => u32::from(b'z'),
        KeyboardKey::Digit0 => u32::from(b'0'),
        KeyboardKey::Digit1 => u32::from(b'1'),
        KeyboardKey::Digit2 => u32::from(b'2'),
        KeyboardKey::Digit3 => u32::from(b'3'),
        KeyboardKey::Digit4 => u32::from(b'4'),
        KeyboardKey::Digit5 => u32::from(b'5'),
        KeyboardKey::Digit6 => u32::from(b'6'),
        KeyboardKey::Digit7 => u32::from(b'7'),
        KeyboardKey::Digit8 => u32::from(b'8'),
        KeyboardKey::Digit9 => u32::from(b'9'),
        KeyboardKey::F1 => XK_F1,
        KeyboardKey::F2 => XK_F1 + 1,
        KeyboardKey::F3 => XK_F1 + 2,
        KeyboardKey::F4 => XK_F1 + 3,
        KeyboardKey::F5 => XK_F1 + 4,
        KeyboardKey::F6 => XK_F1 + 5,
        KeyboardKey::F7 => XK_F1 + 6,
        KeyboardKey::F8 => XK_F1 + 7,
        KeyboardKey::F9 => XK_F1 + 8,
        KeyboardKey::F10 => XK_F1 + 9,
        KeyboardKey::F11 => XK_F1 + 10,
        KeyboardKey::F12 => XK_F1 + 11,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xkb::KTMapEntry;

    fn mask(bits: u8) -> ModMask {
        bits.into()
    }

    fn key_type(mask_bits: u8, levels: &[(u8, u8)]) -> KeyType {
        KeyType {
            mods_mask: mask(mask_bits),
            mods_mods: mask(mask_bits),
            num_levels: levels.len().saturating_add(1).try_into().unwrap_or(u8::MAX),
            map: levels
                .iter()
                .map(|(modifier, level)| KTMapEntry {
                    active: true,
                    mods_mask: mask(*modifier),
                    level: *level,
                    mods_mods: mask(*modifier),
                    ..KTMapEntry::default()
                })
                .collect(),
            ..KeyType::default()
        }
    }

    fn clear_state() -> GetStateReply {
        GetStateReply {
            device_id: 3,
            sequence: 0,
            length: 0,
            mods: ModMask::default(),
            base_mods: ModMask::default(),
            latched_mods: ModMask::default(),
            locked_mods: ModMask::default(),
            group: Default::default(),
            locked_group: Default::default(),
            base_group: 0,
            latched_group: 0,
            compat_state: ModMask::default(),
            grab_mods: ModMask::default(),
            compat_grab_mods: ModMask::default(),
            lookup_mods: ModMask::default(),
            compat_lookup_mods: ModMask::default(),
            ptr_btn_state: Default::default(),
        }
    }

    #[test]
    fn xkb_type_selects_shift_and_level_three_levels() {
        let shift = 1;
        let level_three = 1 << 7;
        let four_level = key_type(
            shift | level_three,
            &[(shift, 1), (level_three, 2), (shift | level_three, 3)],
        );
        assert_eq!(level_for_modifiers(&four_level, 0), 0);
        assert_eq!(level_for_modifiers(&four_level, shift), 1);
        assert_eq!(level_for_modifiers(&four_level, level_three), 2);
        assert_eq!(level_for_modifiers(&four_level, shift | level_three), 3);
    }

    #[test]
    fn xkb_group_overflow_actions_are_bounded() {
        assert_eq!(normalized_group(2, 3), Some(1));
        assert_eq!(normalized_group(0x40 | 2, 3), Some(1));
        assert_eq!(normalized_group(0x80 | 0x10 | 2, 3), Some(1));
        assert_eq!(normalized_group(0xc0 | 2, 3), None);
        assert_eq!(normalized_group(0, 0), None);
    }

    #[test]
    fn requested_modifiers_participate_in_final_symbol_resolution() {
        let control = 1 << 2;
        let map = KeyboardMap {
            minimum: 8,
            types: vec![key_type(control, &[(control, 1)])],
            symbols: vec![
                KeySymMap {
                    kt_index: [0; 4],
                    group_info: 1,
                    width: 1,
                    syms: vec![XK_CONTROL_L],
                },
                KeySymMap {
                    kt_index: [0; 4],
                    group_info: 1,
                    width: 2,
                    syms: vec![u32::from(b'x'), u32::from(b'l')],
                },
            ],
            modifier_map: vec![KeyModMap {
                keycode: 8,
                mods: mask(control),
            }],
            state: clear_state(),
        };

        assert!(map.resolve(u32::from(b'l')).is_err());
        let stroke = match map.resolve_with_modifiers(u32::from(b'l'), &[KeyboardModifier::Control])
        {
            Ok(stroke) => stroke,
            Err(_) => panic!("control did not select the requested symbol"),
        };
        assert_eq!(
            stroke,
            KeyStroke {
                keycode: 9,
                modifiers: vec![8],
            }
        );
    }
}
