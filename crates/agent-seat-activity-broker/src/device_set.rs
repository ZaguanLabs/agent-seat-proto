//! Strict private deployment record for reviewed relevant input devices.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::MAX_EVENT_DESCRIPTORS;

const HEADER: &str = "agent-seat-device-set\t1\n";
const MAX_VALUE_BYTES: usize = 1024;
const MAX_CAPABILITY_VALUE_BYTES: usize = 4096;

/// Maximum encoded size of one reviewed device-set record.
pub const MAX_DEVICE_SET_BYTES: usize = 64 * 1024;

/// Whether identity includes a device-supplied serial or only physical topology.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityStrength {
    /// The device exposes no serial; identical replacement at the same path is ambiguous.
    Topology,
    /// The device exposes `ID_SERIAL_SHORT` in addition to its topology.
    Serial,
}

/// Bounded udev identity evidence retained for one relevant event device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceIdentity {
    path: String,
    bus: Option<String>,
    vendor: Option<String>,
    model: Option<String>,
    revision: Option<String>,
    serial: Option<String>,
}

impl DeviceIdentity {
    /// Parses already-selected udev identity properties into a bounded value.
    pub fn new(
        path: String,
        bus: Option<String>,
        vendor: Option<String>,
        model: Option<String>,
        revision: Option<String>,
        serial: Option<String>,
    ) -> Result<Self, DeviceSetError> {
        validate_value(&path)?;
        for value in [&bus, &vendor, &model, &revision, &serial]
            .into_iter()
            .flatten()
        {
            validate_value(value)?;
        }
        if vendor.is_some() != model.is_some() {
            return Err(DeviceSetError::Invalid);
        }
        Ok(Self {
            path,
            bus,
            vendor,
            model,
            revision,
            serial,
        })
    }

    /// Returns the strongest identity evidence present in this record.
    #[must_use]
    pub const fn strength(&self) -> IdentityStrength {
        if self.serial.is_some() {
            IdentityStrength::Serial
        } else {
            IdentityStrength::Topology
        }
    }
}

/// Canonical kernel input capability bitmaps for one event device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceCapabilities {
    values: [String; 10],
}

/// One fixed kernel capability bitmap in [`DeviceCapabilities`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityKind {
    /// Absolute axes.
    Absolute,
    /// Event types.
    Event,
    /// Force-feedback effects.
    ForceFeedback,
    /// Keys and buttons.
    Key,
    /// LEDs.
    Led,
    /// Miscellaneous event codes.
    Misc,
    /// Relative axes.
    Relative,
    /// Sound codes.
    Sound,
    /// Switch codes.
    Switch,
    /// Input properties.
    Property,
}

impl DeviceCapabilities {
    /// Parses `abs`, `ev`, `ff`, `key`, `led`, `msc`, `rel`, `snd`, `sw`, and
    /// `properties` values in that fixed order.
    pub fn new(values: [String; 10]) -> Result<Self, DeviceSetError> {
        for value in &values {
            validate_capability_value(value)?;
        }
        Ok(Self { values })
    }

    /// Returns whether an evdev code iterator exactly matches one bitmap.
    pub fn matches(&self, kind: CapabilityKind, actual: impl IntoIterator<Item = u16>) -> bool {
        let index = match kind {
            CapabilityKind::Absolute => 0,
            CapabilityKind::Event => 1,
            CapabilityKind::ForceFeedback => 2,
            CapabilityKind::Key => 3,
            CapabilityKind::Led => 4,
            CapabilityKind::Misc => 5,
            CapabilityKind::Relative => 6,
            CapabilityKind::Sound => 7,
            CapabilityKind::Switch => 8,
            CapabilityKind::Property => 9,
        };
        bitmap_codes(&self.values[index]) == actual.into_iter().collect::<Vec<_>>()
    }
}

/// One relevant event device and the exact evidence reviewed for enrollment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnrolledDevice {
    event_number: u32,
    sysfs_path: PathBuf,
    classes: Vec<String>,
    identity: DeviceIdentity,
    capabilities: DeviceCapabilities,
}

impl EnrolledDevice {
    /// Constructs one strictly validated reviewed device record.
    pub fn new(
        event_number: u32,
        sysfs_path: PathBuf,
        classes: Vec<String>,
        identity: DeviceIdentity,
        capabilities: DeviceCapabilities,
    ) -> Result<Self, DeviceSetError> {
        validate_sysfs_path(event_number, &sysfs_path)?;
        validate_classes(&classes)?;
        Ok(Self {
            event_number,
            sysfs_path,
            classes,
            identity,
            capabilities,
        })
    }

    /// Returns the reviewed kernel event number.
    #[must_use]
    pub const fn event_number(&self) -> u32 {
        self.event_number
    }

    /// Returns the exact reviewed evdev capability evidence.
    #[must_use]
    pub const fn capabilities(&self) -> &DeviceCapabilities {
        &self.capabilities
    }
}

/// Error returned for malformed or over-bound reviewed device records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceSetError {
    /// The record violates canonical syntax or an identity invariant.
    Invalid,
    /// The encoded record exceeds its fixed byte or device-count bound.
    TooLarge,
}

impl fmt::Display for DeviceSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("invalid reviewed device set"),
            Self::TooLarge => formatter.write_str("reviewed device set exceeds its bound"),
        }
    }
}

impl std::error::Error for DeviceSetError {}

/// Encodes a sorted, bounded reviewed relevant-device record.
pub fn encode_device_set(devices: &[EnrolledDevice]) -> Result<Vec<u8>, DeviceSetError> {
    if devices.is_empty() || devices.len() > MAX_EVENT_DESCRIPTORS {
        return Err(DeviceSetError::TooLarge);
    }
    let mut output = String::with_capacity(HEADER.len() + devices.len() * 256);
    output.push_str(HEADER);
    let mut previous = None;
    for device in devices {
        if previous.is_some_and(|number| number >= device.event_number) {
            return Err(DeviceSetError::Invalid);
        }
        previous = Some(device.event_number);
        validate_sysfs_path(device.event_number, &device.sysfs_path)?;
        validate_classes(&device.classes)?;
        use std::fmt::Write as _;
        writeln!(
            output,
            "event{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            device.event_number,
            device.sysfs_path.display(),
            device.classes.join(","),
            device.identity.path,
            optional(&device.identity.bus),
            optional(&device.identity.vendor),
            optional(&device.identity.model),
            optional(&device.identity.revision),
            optional(&device.identity.serial),
            device.capabilities.values[0],
            device.capabilities.values[1],
            device.capabilities.values[2],
            device.capabilities.values[3],
            device.capabilities.values[4],
            device.capabilities.values[5],
            device.capabilities.values[6],
            device.capabilities.values[7],
            device.capabilities.values[8],
            device.capabilities.values[9],
        )
        .map_err(|_| DeviceSetError::Invalid)?;
        if output.len() > MAX_DEVICE_SET_BYTES {
            return Err(DeviceSetError::TooLarge);
        }
    }
    Ok(output.into_bytes())
}

/// Decodes and canonicalizes one reviewed relevant-device record.
pub fn decode_device_set(bytes: &[u8]) -> Result<Vec<EnrolledDevice>, DeviceSetError> {
    if bytes.len() > MAX_DEVICE_SET_BYTES {
        return Err(DeviceSetError::TooLarge);
    }
    let source = std::str::from_utf8(bytes).map_err(|_| DeviceSetError::Invalid)?;
    let body = source.strip_prefix(HEADER).ok_or(DeviceSetError::Invalid)?;
    if body.is_empty() || !body.ends_with('\n') {
        return Err(DeviceSetError::Invalid);
    }
    let mut devices = Vec::new();
    for line in body.lines() {
        if devices.len() >= MAX_EVENT_DESCRIPTORS {
            return Err(DeviceSetError::TooLarge);
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        let [
            event,
            sysfs,
            classes,
            path,
            bus,
            vendor,
            model,
            revision,
            serial,
            abs,
            ev,
            ff,
            key,
            led,
            msc,
            rel,
            snd,
            sw,
            properties,
        ] = fields.as_slice()
        else {
            return Err(DeviceSetError::Invalid);
        };
        let number = canonical_event_number(event)?;
        let identity = DeviceIdentity::new(
            (*path).to_owned(),
            parse_optional(bus)?,
            parse_optional(vendor)?,
            parse_optional(model)?,
            parse_optional(revision)?,
            parse_optional(serial)?,
        )?;
        let classes = classes.split(',').map(str::to_owned).collect();
        let capabilities = DeviceCapabilities::new([
            (*abs).to_owned(),
            (*ev).to_owned(),
            (*ff).to_owned(),
            (*key).to_owned(),
            (*led).to_owned(),
            (*msc).to_owned(),
            (*rel).to_owned(),
            (*snd).to_owned(),
            (*sw).to_owned(),
            (*properties).to_owned(),
        ])?;
        devices.push(EnrolledDevice::new(
            number,
            PathBuf::from(sysfs),
            classes,
            identity,
            capabilities,
        )?);
    }
    let canonical = encode_device_set(&devices)?;
    if canonical != bytes {
        return Err(DeviceSetError::Invalid);
    }
    Ok(devices)
}

fn optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("-")
}

fn parse_optional(value: &str) -> Result<Option<String>, DeviceSetError> {
    if value == "-" {
        Ok(None)
    } else {
        validate_value(value)?;
        Ok(Some(value.to_owned()))
    }
}

fn validate_value(value: &str) -> Result<(), DeviceSetError> {
    if value.is_empty()
        || value == "-"
        || value.len() > MAX_VALUE_BYTES
        || !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(DeviceSetError::Invalid);
    }
    Ok(())
}

fn validate_capability_value(value: &str) -> Result<(), DeviceSetError> {
    if value.is_empty()
        || value.len() > MAX_CAPABILITY_VALUE_BYTES
        || value.starts_with(' ')
        || value.ends_with(' ')
        || value.contains("  ")
        || !value
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DeviceSetError::Invalid);
    }
    Ok(())
}

fn bitmap_codes(value: &str) -> Vec<u16> {
    let mut codes = Vec::new();
    for (word_index, word) in value.split(' ').rev().enumerate() {
        let mut word = usize::from_str_radix(word, 16).unwrap_or_default();
        let base = word_index.saturating_mul(usize::BITS as usize);
        while word != 0 {
            let bit = word.trailing_zeros() as usize;
            if let Ok(code) = u16::try_from(base.saturating_add(bit)) {
                codes.push(code);
            }
            word &= word - 1;
        }
    }
    codes.sort_unstable();
    codes
}

fn canonical_event_number(value: &str) -> Result<u32, DeviceSetError> {
    let digits = value.strip_prefix("event").ok_or(DeviceSetError::Invalid)?;
    let number = digits.parse::<u32>().map_err(|_| DeviceSetError::Invalid)?;
    if number.to_string() != digits {
        return Err(DeviceSetError::Invalid);
    }
    Ok(number)
}

fn validate_sysfs_path(event_number: u32, path: &Path) -> Result<(), DeviceSetError> {
    let expected_suffix = format!("event{event_number}");
    if !path.is_absolute()
        || !path.starts_with("/sys/devices")
        || path.file_name().and_then(|name| name.to_str()) != Some(&expected_suffix)
        || !path.components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
    {
        return Err(DeviceSetError::Invalid);
    }
    Ok(())
}

fn validate_classes(classes: &[String]) -> Result<(), DeviceSetError> {
    if classes.is_empty() || classes.len() > 6 {
        return Err(DeviceSetError::Invalid);
    }
    let allowed = [
        "key",
        "keyboard",
        "mouse",
        "touchpad",
        "touchscreen",
        "tablet",
    ];
    let mut seen = BTreeSet::new();
    let mut previous = None;
    for class in classes {
        let Some(position) = allowed.iter().position(|allowed| *allowed == class) else {
            return Err(DeviceSetError::Invalid);
        };
        if previous.is_some_and(|previous| previous >= position) || !seen.insert(class) {
            return Err(DeviceSetError::Invalid);
        }
        previous = Some(position);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(serial: Option<&str>) -> EnrolledDevice {
        EnrolledDevice::new(
            2,
            PathBuf::from("/sys/devices/example/input/input2/event2"),
            vec!["key".to_owned(), "keyboard".to_owned()],
            DeviceIdentity::new(
                "pci-0000:00:01.0-usb-0:1:1.0".to_owned(),
                Some("usb".to_owned()),
                Some("046d".to_owned()),
                Some("c548".to_owned()),
                Some("0504".to_owned()),
                serial.map(str::to_owned),
            )
            .expect("identity fixture"),
            DeviceCapabilities::new([
                "0".to_owned(),
                "120013".to_owned(),
                "0".to_owned(),
                "1000000000007 ff9f207ac14057ff".to_owned(),
                "1f".to_owned(),
                "10".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
            ])
            .expect("capability fixture"),
        )
        .expect("device fixture")
    }

    #[test]
    fn device_sets_round_trip_and_distinguish_identity_strength() {
        let topology = device(None);
        assert_eq!(topology.identity.strength(), IdentityStrength::Topology);
        let serial = device(Some("receiver-1"));
        assert_eq!(serial.identity.strength(), IdentityStrength::Serial);
        let encoded = encode_device_set(std::slice::from_ref(&serial)).expect("encode fixture");
        assert_eq!(decode_device_set(&encoded), Ok(vec![serial]));
        assert!(
            topology
                .capabilities
                .matches(CapabilityKind::Event, [0, 1, 4, 17, 20])
        );
    }

    #[test]
    fn device_sets_reject_noncanonical_or_ambiguous_identity() {
        let canonical = encode_device_set(&[device(None)]).expect("encode fixture");
        let mut trailing = canonical.clone();
        trailing.push(b'\n');
        assert_eq!(decode_device_set(&trailing), Err(DeviceSetError::Invalid));
        assert!(
            DeviceIdentity::new(
                "-".to_owned(),
                None,
                Some("046d".to_owned()),
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(DeviceCapabilities::new(std::array::from_fn(|_| "0A".to_owned())).is_err());
    }
}
