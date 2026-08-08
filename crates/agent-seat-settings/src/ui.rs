//! GTK interface; policy behavior remains in the display-independent model.

use std::path::PathBuf;

use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};

use agent_seat_settings::SettingsModel;

const APPLICATION_ID: &str = "org.zaguanlabs.AgentSeat.Settings";

pub(crate) fn run(config: Option<PathBuf>) -> Result<(), String> {
    let application = Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.connect_activate(move |application| {
        let result = match config.as_deref() {
            Some(path) => SettingsModel::open(path).map(|model| (model, false)),
            None => SettingsModel::open_default(),
        };
        match result {
            Ok((model, created)) => build_window(application, model, created),
            Err(error) => build_error_window(application, &error),
        }
    });
    let _ = application.run();
    Ok(())
}

fn build_window(application: &Application, model: SettingsModel, created: bool) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Agent Seat Settings")
        .default_width(980)
        .default_height(720)
        .build();
    let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    page.set_margin_top(32);
    page.set_margin_end(32);
    page.set_margin_bottom(32);
    page.set_margin_start(32);
    let title = gtk::Label::builder()
        .label("Agent Seat policy")
        .xalign(0.0)
        .css_classes(["title-1"])
        .build();
    let path = gtk::Label::builder()
        .label(model.path().to_string_lossy())
        .xalign(0.0)
        .selectable(true)
        .css_classes(["dim-label", "monospace"])
        .build();
    let state = gtk::Label::builder()
        .label(if created {
            "Saved · valid and disabled    Draft · matches saved    Active · not inspected"
        } else {
            "Saved · valid    Draft · matches saved    Active · not inspected"
        })
        .xalign(0.0)
        .wrap(true)
        .build();
    let guidance = gtk::Label::builder()
        .label(
            "The documented policy is ready. The complete editor controls are being loaded from the provider-owned settings model.",
        )
        .xalign(0.0)
        .wrap(true)
        .build();
    page.append(&title);
    page.append(&path);
    page.append(&state);
    page.append(&guidance);
    window.set_child(Some(&page));
    window.present();
}

fn build_error_window(application: &Application, error: &str) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Agent Seat Settings")
        .default_width(620)
        .default_height(280)
        .build();
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_margin_top(32);
    page.set_margin_end(32);
    page.set_margin_bottom(32);
    page.set_margin_start(32);
    page.append(
        &gtk::Label::builder()
            .label("Policy could not be opened")
            .xalign(0.0)
            .css_classes(["title-2"])
            .build(),
    );
    page.append(
        &gtk::Label::builder()
            .label(error)
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .build(),
    );
    let close = gtk::Button::with_label("Close");
    let weak_window = window.downgrade();
    close.connect_clicked(move |_| {
        if let Some(window) = weak_window.upgrade() {
            window.close();
        }
    });
    page.append(&close);
    window.set_child(Some(&page));
    window.present();
}
