//! Explicit no-event uinput fixture for the installed T5 hotplug gate.

#![forbid(unsafe_code)]

use std::io::{self, Write as _};
use std::thread;
use std::time::Duration;

use evdev::{AttributeSet, RelativeAxisCode, uinput::VirtualDevice};

fn main() -> io::Result<()> {
    let axes = AttributeSet::from_iter([RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y]);
    let device = VirtualDevice::builder()?
        .name("Agent Seat T5 no-event hotplug fixture")
        .with_relative_axes(&axes)?
        .build()?;

    println!("agent-seat-hotplug-fixture=ready");
    io::stdout().flush()?;
    thread::sleep(Duration::from_secs(2));
    drop(device);
    println!("agent-seat-hotplug-fixture=removed");
    Ok(())
}
