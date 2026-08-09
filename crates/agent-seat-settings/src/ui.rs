//! GTK interface; policy behavior remains in the display-independent model.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use agent_seat_proto::{ApplicationDescriptor, ApplicationId, Capability};
use agent_seat_settings::SettingsModel;
use agent_seat_x11::{
    ActivePolicyStatus, ClientScope, LaunchMode, MAX_POLICY_IO_TIMEOUT_MS, MAX_POLICY_REQUESTS,
    MAX_POLICY_SESSIONS, MIN_POLICY_IO_TIMEOUT_MS, MIN_POLICY_REQUESTS, MIN_POLICY_SESSIONS,
    RuntimeSeatCommand, RuntimeSeatStatus, control_runtime_seat,
};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};

const APPLICATION_ID: &str = "org.zaguanlabs.AgentSeat.Settings";
const STYLE: &str = r#"
.agent-seat-window { background: #eef2f6; color: #152333; }
.agent-seat-header { padding: 20px 24px 10px; }
.agent-seat-title { font-size: 24px; font-weight: 700; }
.agent-seat-path, .monospace { font-family: monospace; }
.agent-seat-path { opacity: 0.72; }
.state-rail { margin: 8px 24px 18px; padding: 14px 18px; background: #ffffff; border: 1px solid #c9d3de; border-radius: 10px; }
.state-node { padding: 2px 12px; }
.state-name { font-size: 11px; font-weight: 700; letter-spacing: 1px; }
.state-good { color: #2d7d62; }
.state-warning { color: #9a5b00; }
.state-unknown { color: #52677d; }
.state-enabled { color: #225eaa; }
.state-disabled { color: #52677d; }
.sidebar { min-width: 190px; background: #e2e9f0; border-right: 1px solid #c9d3de; }
.page { padding: 24px; }
.page-title { font-size: 22px; font-weight: 700; }
.page-intro { color: #42566b; margin-bottom: 10px; }
.panel { background: #ffffff; border: 1px solid #c9d3de; border-radius: 10px; padding: 16px; margin-bottom: 14px; }
.runtime-seat-panel { border-left: 4px solid #225eaa; }
.panel-title { font-size: 16px; font-weight: 700; }
.panel-description { color: #52677d; margin-bottom: 8px; }
.control-row { padding: 10px 0; border-bottom: 1px solid #e3e8ed; }
.control-title { font-weight: 600; }
.control-description { color: #52677d; font-size: 13px; }
.atom { color: #225eaa; font-family: monospace; font-size: 12px; }
.user-badge { color: #9a5b00; font-size: 11px; font-weight: 700; }
.message { margin: 8px 24px; padding: 10px 14px; border-radius: 8px; }
.message-error { background: #f9d9d6; color: #7d211c; }
.message-success { background: #d8eee5; color: #205c49; }
.footer { padding: 12px 24px 18px; }
.diff-view { font-family: monospace; font-size: 12px; }
"#;

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
    let _ = application.run_with_args(&["agent-seat-settings"]);
    Ok(())
}

struct CapabilityControl {
    capability: Capability,
    button: gtk::CheckButton,
}

struct CatalogControl {
    application: ApplicationDescriptor,
    row: gtk::ListBoxRow,
    button: gtk::CheckButton,
    search_key: String,
}

struct RuntimeSeatControls {
    state: gtk::Label,
    detail: gtk::Label,
    refresh: gtk::Button,
    enable: gtk::Button,
    disable: gtk::Button,
}

struct Controls {
    root: gtk::Box,
    saved_state: gtk::Label,
    draft_state: gtk::Label,
    active_state: gtk::Label,
    runtime: RuntimeSeatControls,
    enabled: gtk::Switch,
    capabilities: Vec<CapabilityControl>,
    clear_grant: gtk::Button,
    scope: gtk::DropDown,
    titles: gtk::Switch,
    launch_mode: gtk::DropDown,
    user_entries: gtk::Switch,
    catalog_search: gtk::SearchEntry,
    catalog_count: gtk::Label,
    catalog: Vec<CatalogControl>,
    catalog_error: gtk::Label,
    max_sessions: gtk::SpinButton,
    max_requests: gtk::SpinButton,
    io_timeout: gtk::SpinButton,
    diff: gtk::TextView,
    validation: gtk::Label,
    message: gtk::Label,
    save: gtk::Button,
    discard: gtk::Button,
    reload: gtk::Button,
    restore: gtk::Button,
}

struct Ui {
    model: RefCell<SettingsModel>,
    controls: Controls,
    window: glib::WeakRef<ApplicationWindow>,
    updating: Cell<bool>,
    allow_close: Cell<bool>,
}

fn build_window(application: &Application, model: SettingsModel, created: bool) {
    install_style();
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Agent Seat Settings")
        .default_width(1080)
        .default_height(780)
        .build();
    window.add_css_class("agent-seat-window");
    let controls = build_controls(&model, created);
    window.set_child(Some(&controls.root));
    let ui = Rc::new(Ui {
        model: RefCell::new(model),
        controls,
        window: window.downgrade(),
        updating: Cell::new(false),
        allow_close: Cell::new(false),
    });
    ui.connect();
    ui.refresh();
    ui.refresh_runtime_seat();
    let keep_alive = Rc::clone(&ui);
    window.connect_close_request(move |_| keep_alive.close_request());
    window.present();
}

fn install_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_controls(model: &SettingsModel, created: bool) -> Controls {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = gtk::Box::new(gtk::Orientation::Vertical, 3);
    header.add_css_class("agent-seat-header");
    let title = label("Agent Seat policy", "agent-seat-title");
    let path = label(&model.path().to_string_lossy(), "agent-seat-path");
    path.set_selectable(true);
    header.append(&title);
    header.append(&path);
    root.append(&header);

    let saved_state = gtk::Label::new(None);
    let draft_state = gtk::Label::new(None);
    let active_state = gtk::Label::new(None);
    let runtime = RuntimeSeatControls {
        state: gtk::Label::new(None),
        detail: label("Checking the selected X11 provider…", "control-description"),
        refresh: gtk::Button::with_label("Refresh status"),
        enable: gtk::Button::with_label("Enable for this instance"),
        disable: gtk::Button::with_label("Disable now"),
    };
    runtime.detail.set_wrap(true);
    runtime.detail.set_selectable(true);
    runtime.enable.add_css_class("suggested-action");
    runtime.disable.add_css_class("destructive-action");
    let rail = state_rail(&saved_state, &draft_state, &active_state, &runtime.state);
    root.append(&rail);

    let enabled = gtk::Switch::new();
    enabled.set_valign(gtk::Align::Center);
    let reload = gtk::Button::with_label("Reload saved policy");
    let restore = gtk::Button::with_label("Restore previous policy");
    restore.add_css_class("destructive-action");
    let overview = overview_page(model, created, &enabled, &runtime, &reload, &restore);

    let (access, capabilities, clear_grant) = access_page();

    let scope = gtk::DropDown::from_strings(&["No windows", "Current workspace", "All workspaces"]);
    let titles = gtk::Switch::new();
    titles.set_valign(gtk::Align::Center);
    let visibility = visibility_page(&scope, &titles);

    let launch_mode = gtk::DropDown::from_strings(&[
        "Deny every application",
        "Allow selected applications",
        "Allow installed applications except denied",
    ]);
    let user_entries = gtk::Switch::new();
    user_entries.set_valign(gtk::Align::Center);
    let catalog_search = gtk::SearchEntry::builder()
        .placeholder_text("Search by application name or desktop ID")
        .build();
    let catalog_count = label("", "control-description");
    let catalog_error = label("", "control-description");
    catalog_error.set_wrap(true);
    let catalog_result = model.application_catalog();
    let (applications, catalog) = applications_page(
        &launch_mode,
        &user_entries,
        &catalog_search,
        &catalog_count,
        &catalog_error,
        catalog_result,
    );

    let max_sessions = gtk::SpinButton::with_range(
        f64::from(MIN_POLICY_SESSIONS),
        f64::from(MAX_POLICY_SESSIONS),
        1.0,
    );
    let max_requests = gtk::SpinButton::with_range(
        f64::from(MIN_POLICY_REQUESTS),
        f64::from(MAX_POLICY_REQUESTS),
        1.0,
    );
    let io_timeout = gtk::SpinButton::with_range(
        f64::from(MIN_POLICY_IO_TIMEOUT_MS),
        f64::from(MAX_POLICY_IO_TIMEOUT_MS),
        50.0,
    );
    let limits = limits_page(&max_sessions, &max_requests, &io_timeout);

    let diff = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .build();
    diff.add_css_class("diff-view");
    let validation = label("", "control-description");
    let review = review_page(&diff, &validation);

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hexpand(true)
        .vexpand(true)
        .build();
    for (name, title, page) in [
        ("overview", "Overview", overview),
        ("access", "Access", access),
        ("visibility", "Visible windows", visibility),
        ("applications", "Applications", applications),
        ("limits", "Limits", limits),
        ("review", "Review", review),
    ] {
        stack.add_titled(&page, Some(name), title);
    }
    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.add_css_class("sidebar");
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.set_vexpand(true);
    body.append(&sidebar);
    body.append(&stack);
    root.append(&body);

    let message = label("", "message");
    message.set_wrap(true);
    message.set_visible(false);
    root.append(&message);

    let save = gtk::Button::with_label("Save changes");
    save.add_css_class("suggested-action");
    let discard = gtk::Button::with_label("Discard draft");
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.add_css_class("footer");
    footer.set_halign(gtk::Align::End);
    footer.append(&discard);
    footer.append(&save);
    root.append(&footer);

    Controls {
        root,
        saved_state,
        draft_state,
        active_state,
        runtime,
        enabled,
        capabilities,
        clear_grant,
        scope,
        titles,
        launch_mode,
        user_entries,
        catalog_search,
        catalog_count,
        catalog,
        catalog_error,
        max_sessions,
        max_requests,
        io_timeout,
        diff,
        validation,
        message,
        save,
        discard,
        reload,
        restore,
    }
}

fn state_rail(
    saved: &gtk::Label,
    draft: &gtk::Label,
    active: &gtk::Label,
    runtime: &gtk::Label,
) -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    rail.add_css_class("state-rail");
    rail.set_homogeneous(true);
    for (name, status) in [
        ("SAVED", saved),
        ("DRAFT", draft),
        ("ACTIVE POLICY", active),
        ("RUNTIME SEAT", runtime),
    ] {
        let node = gtk::Box::new(gtk::Orientation::Vertical, 3);
        node.add_css_class("state-node");
        node.append(&label(name, "state-name"));
        status.set_xalign(0.0);
        status.set_wrap(true);
        node.append(status);
        rail.append(&node);
    }
    rail
}

fn overview_page(
    model: &SettingsModel,
    created: bool,
    enabled: &gtk::Switch,
    runtime_controls: &RuntimeSeatControls,
    reload: &gtk::Button,
    restore: &gtk::Button,
) -> gtk::ScrolledWindow {
    let (content, page) = page(
        "Overview",
        "Keep saved policy and the current provider's volatile seat visibly separate.",
    );

    let runtime = panel(
        "Current provider instance",
        "The runtime seat starts disabled after every provider start. Enabling lasts only until that provider or its X11 display exits; saving policy never enables it.",
    );
    runtime.add_css_class("runtime-seat-panel");
    runtime.append(&control_row(
        "Runtime seat",
        "Status follows the live provider advertised on this X11 screen.",
        &runtime_controls.detail,
    ));
    let runtime_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    runtime_actions.set_margin_top(10);
    runtime_actions.append(&runtime_controls.refresh);
    runtime_actions.append(&runtime_controls.enable);
    runtime_actions.append(&runtime_controls.disable);
    runtime.append(&runtime_actions);
    content.append(&runtime);

    let activation = panel(
        "Saved provider policy",
        "This controls whether reviewed policy may start. It does not change the runtime seat above.",
    );
    activation.append(&control_row(
        "Enable provider policy",
        "A running provider uses this only after it is restarted.",
        enabled,
    ));
    content.append(&activation);

    let files = panel(
        "Policy files",
        "Every read and write uses the provider's ownership and mode checks.",
    );
    files.append(&value_row("Saved policy", &model.path().to_string_lossy()));
    files.append(&value_row(
        "Recovery policy",
        &model.recovery_path().to_string_lossy(),
    ));
    if created {
        files.append(&label(
            "A documented, private, disabled policy was created for this first run.",
            "control-description",
        ));
    }
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_margin_top(10);
    actions.append(reload);
    actions.append(restore);
    files.append(&actions);
    content.append(&files);
    page
}

fn access_page() -> (gtk::ScrolledWindow, Vec<CapabilityControl>, gtk::Button) {
    let (content, page) = page(
        "Access",
        "Grant only the actions the agent needs. Prerequisites are shown and are never enabled silently.",
    );
    let mut controls = Vec::new();
    for (group, description, specs) in capability_groups() {
        let section = panel(group, description);
        for spec in specs {
            let button = gtk::CheckButton::new();
            button.set_valign(gtk::Align::Center);
            let detail = if spec.dependency.is_empty() {
                spec.description.to_owned()
            } else {
                format!("{} Requires {}.", spec.description, spec.dependency)
            };
            let row = control_row(spec.title, &detail, &button);
            if let Some(left) = row.first_child().and_downcast::<gtk::Box>() {
                left.append(&label(spec.atom, "atom"));
            }
            section.append(&row);
            controls.push(CapabilityControl {
                capability: spec.capability,
                button,
            });
        }
        content.append(&section);
    }
    let clear = gtk::Button::with_label("Remove all access");
    clear.add_css_class("destructive-action");
    content.append(&clear);
    (page, controls, clear)
}

fn visibility_page(scope: &gtk::DropDown, titles: &gtk::Switch) -> gtk::ScrolledWindow {
    let (content, page) = page(
        "Visible windows",
        "Limit which clients are visible and separately control title text.",
    );
    let section = panel(
        "Observation scope",
        "Window structure must also be granted on the Access page.",
    );
    section.append(&control_row(
        "Visible clients",
        "Choose no clients, the current workspace, or every workspace.",
        scope,
    ));
    section.append(&control_row(
        "Expose window titles",
        "Both this switch and the title-access capability must be enabled.",
        titles,
    ));
    content.append(&section);
    page
}

fn applications_page(
    mode: &gtk::DropDown,
    user_entries: &gtk::Switch,
    search: &gtk::SearchEntry,
    count: &gtk::Label,
    error: &gtk::Label,
    catalog_result: Result<Vec<ApplicationDescriptor>, String>,
) -> (gtk::ScrolledWindow, Vec<CatalogControl>) {
    let (content, page) = page(
        "Applications",
        "Choose launchable desktop entries by localized name while keeping canonical desktop IDs visible.",
    );
    let policy = panel(
        "Launch policy",
        "Application listing and launch capabilities are granted separately on the Access page.",
    );
    policy.append(&control_row(
        "Admission mode",
        "Deny all, select an allow-list, or allow installed entries except explicit denials.",
        mode,
    ));
    policy.append(&control_row(
        "Allow user entries",
        "Entries under your user data directory are writable and remain separately denied by default.",
        user_entries,
    ));
    content.append(&policy);

    let catalog_panel = panel(
        "Launchable application catalog",
        "This is the provider's bounded parser result, not a generic desktop menu.",
    );
    catalog_panel.append(search);
    catalog_panel.append(count);
    catalog_panel.append(error);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let mut controls = Vec::new();
    match catalog_result {
        Ok(applications) => {
            for application in applications {
                let button = gtk::CheckButton::new();
                button.set_valign(gtk::Align::Center);
                let row = gtk::ListBoxRow::new();
                let layout = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                layout.set_margin_top(8);
                layout.set_margin_end(8);
                layout.set_margin_bottom(8);
                layout.set_margin_start(8);
                layout.append(&button);
                let names = gtk::Box::new(gtk::Orientation::Vertical, 2);
                names.set_hexpand(true);
                names.append(&label(&application.name, "control-title"));
                names.append(&label(&application.id, "atom"));
                layout.append(&names);
                if application.user_entry {
                    layout.append(&label("USER ENTRY", "user-badge"));
                }
                row.set_child(Some(&layout));
                let search_key = format!("{} {}", application.name, application.id).to_lowercase();
                list.append(&row);
                controls.push(CatalogControl {
                    application,
                    row,
                    button,
                    search_key,
                });
            }
        }
        Err(message) => {
            error.set_text(&format!("Catalog unavailable: {message}"));
            error.add_css_class("message-error");
        }
    }
    let scroller = gtk::ScrolledWindow::builder()
        .min_content_height(340)
        .vexpand(true)
        .child(&list)
        .build();
    catalog_panel.append(&scroller);
    content.append(&catalog_panel);
    (page, controls)
}

fn limits_page(
    sessions: &gtk::SpinButton,
    requests: &gtk::SpinButton,
    timeout: &gtk::SpinButton,
) -> gtk::ScrolledWindow {
    let (content, page) = page(
        "Limits",
        "Keep resource use explicit and bounded. Defaults suit an ordinary single-user desktop.",
    );
    let section = panel(
        "Provider limits",
        "Invalid values are refused before saving.",
    );
    section.append(&control_row(
        "Concurrent sessions",
        "Accepted range: 1–32.",
        sessions,
    ));
    section.append(&control_row(
        "Requests per session",
        "Accepted range: 1–4096.",
        requests,
    ));
    section.append(&control_row(
        "I/O timeout (milliseconds)",
        "Accepted range: 50–10000.",
        timeout,
    ));
    content.append(&section);
    page
}

fn review_page(diff: &gtk::TextView, validation: &gtk::Label) -> gtk::ScrolledWindow {
    let (content, page) = page(
        "Review",
        "Inspect the exact source changes. Saving refuses stale files and retains the previous private policy.",
    );
    let section = panel(
        "Before / after policy diff",
        "Lines beginning with − are removed; lines beginning with + are added.",
    );
    section.append(validation);
    let scroller = gtk::ScrolledWindow::builder()
        .min_content_height(440)
        .vexpand(true)
        .child(diff)
        .build();
    section.append(&scroller);
    content.append(&section);
    page
}

#[derive(Debug, Eq, PartialEq)]
struct RuntimeSeatPresentation {
    rail: String,
    detail: String,
    class: &'static str,
    can_enable: bool,
    can_disable: bool,
}

fn runtime_seat_presentation(
    result: &Result<RuntimeSeatStatus, String>,
) -> RuntimeSeatPresentation {
    match result {
        Ok(status) => known_runtime_seat_presentation(status.is_enabled(), status.generation()),
        Err(error) => RuntimeSeatPresentation {
            rail: "Unavailable · denied".to_owned(),
            detail: format!(
                "Runtime status could not be verified, so access stays denied. {error}"
            ),
            class: "state-unknown",
            can_enable: false,
            can_disable: false,
        },
    }
}

fn known_runtime_seat_presentation(enabled: bool, generation: u64) -> RuntimeSeatPresentation {
    if enabled {
        RuntimeSeatPresentation {
            rail: format!("Enabled · generation {generation}"),
            detail: format!(
                "Enabled for the current provider instance at generation {generation}."
            ),
            class: "state-enabled",
            can_enable: false,
            can_disable: true,
        }
    } else {
        RuntimeSeatPresentation {
            rail: format!("Disabled · generation {generation}"),
            detail: format!(
                "Disabled at generation {generation}. New Agent Seat sessions are denied."
            ),
            class: "state-disabled",
            can_enable: true,
            can_disable: false,
        }
    }
}

impl Ui {
    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.controls.enabled.connect_active_notify(move |button| {
            apply_weak(&weak, |draft| {
                draft.set_enabled(button.is_active());
                Ok(())
            });
        });

        let weak = Rc::downgrade(self);
        self.controls.runtime.refresh.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh_runtime_seat();
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.runtime.enable.connect_clicked(move |_| {
            let weak_action = Weak::clone(&weak);
            if let Some(ui) = weak.upgrade() {
                ui.confirm(
                    "Enable the current runtime seat?",
                    "New Agent Seat sessions may use the running provider until you disable it or that provider exits. Saved policy does not change.",
                    "Enable for this instance",
                    false,
                    move || {
                        if let Some(ui) = weak_action.upgrade() {
                            ui.change_runtime_seat(RuntimeSeatCommand::Enable);
                        }
                    },
                );
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.runtime.disable.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.change_runtime_seat(RuntimeSeatCommand::Disable);
            }
        });

        for control in &self.controls.capabilities {
            let weak = Rc::downgrade(self);
            let capability = control.capability;
            control.button.connect_toggled(move |button| {
                let active = button.is_active();
                apply_weak(&weak, move |draft| {
                    let mut capabilities = draft.capabilities().to_vec();
                    if active && !capabilities.contains(&capability) {
                        capabilities.push(capability);
                    } else if !active {
                        capabilities.retain(|entry| *entry != capability);
                    }
                    draft.set_capabilities(capabilities)
                });
            });
        }

        let weak = Rc::downgrade(self);
        self.controls.clear_grant.connect_clicked(move |_| {
            let weak_action = Weak::clone(&weak);
            if let Some(ui) = weak.upgrade() {
                ui.confirm(
                    "Remove all access?",
                    "The draft grant and every capability will be removed. Nothing changes on disk until you save.",
                    "Remove all access",
                    true,
                    move || {
                        apply_weak(&weak_action, |draft| {
                            draft.clear_grant();
                            Ok(())
                        });
                    },
                );
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.scope.connect_selected_notify(move |scope| {
            let selected = scope.selected();
            apply_weak(&weak, move |draft| {
                let clients = match selected {
                    0 => ClientScope::None,
                    1 => ClientScope::CurrentWorkspace,
                    2 => ClientScope::AllWorkspaces,
                    _ => return Err("unknown observation scope selection".to_owned()),
                };
                let (_, titles) = draft.observation();
                draft.set_observation(clients, titles)
            });
        });

        let weak = Rc::downgrade(self);
        self.controls.titles.connect_active_notify(move |button| {
            let titles = button.is_active();
            apply_weak(&weak, move |draft| {
                let (clients, _) = draft.observation();
                draft.set_observation(clients, titles)
            });
        });

        let weak = Rc::downgrade(self);
        self.controls
            .launch_mode
            .connect_selected_notify(move |mode| {
                let selected = mode.selected();
                let mode = match selected {
                    0 => Ok(LaunchMode::Deny),
                    1 => Ok(LaunchMode::AllowListed),
                    2 => Ok(LaunchMode::AllowInstalled),
                    _ => Err("unknown launch mode selection".to_owned()),
                };
                if let Some(ui) = weak.upgrade() {
                    match mode {
                        Ok(mode) => ui.change_launch_mode(mode),
                        Err(error) => ui.error(&error),
                    }
                }
            });

        let weak = Rc::downgrade(self);
        self.controls
            .user_entries
            .connect_active_notify(move |button| {
                let allowed = button.is_active();
                apply_weak(&weak, move |draft| {
                    draft.set_launch(
                        draft.launch_mode(),
                        draft.launch_allow().to_vec(),
                        draft.launch_deny().to_vec(),
                        allowed,
                    )
                });
            });

        for control in &self.controls.catalog {
            let weak = Rc::downgrade(self);
            let id = control.application.id.clone();
            control.button.connect_toggled(move |button| {
                let admitted = button.is_active();
                let id = id.clone();
                apply_weak(&weak, move |draft| edit_application(draft, id, admitted));
            });
        }

        let weak = Rc::downgrade(self);
        self.controls
            .catalog_search
            .connect_search_changed(move |search| {
                if let Some(ui) = weak.upgrade() {
                    ui.filter_catalog(&search.text());
                }
            });

        for spinner in [
            &self.controls.max_sessions,
            &self.controls.max_requests,
            &self.controls.io_timeout,
        ] {
            let weak = Rc::downgrade(self);
            spinner.connect_value_changed(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.edit_limits();
                }
            });
        }

        let weak = Rc::downgrade(self);
        self.controls.save.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.confirm_save();
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.discard.connect_clicked(move |_| {
            let weak_action = Weak::clone(&weak);
            if let Some(ui) = weak.upgrade() {
                ui.confirm(
                    "Discard draft changes?",
                    "Every unsaved control will return to the last loaded policy.",
                    "Discard draft",
                    true,
                    move || {
                        if let Some(ui) = weak_action.upgrade() {
                            ui.model.borrow_mut().discard_draft();
                            ui.clear_message();
                            ui.refresh();
                        }
                    },
                );
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.reload.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.reload();
            }
        });

        let weak = Rc::downgrade(self);
        self.controls.restore.connect_clicked(move |_| {
            let weak_action = Weak::clone(&weak);
            if let Some(ui) = weak.upgrade() {
                ui.confirm(
                    "Restore the previous policy?",
                    "The current and .previous policies will be atomically exchanged after both pass strict validation.",
                    "Restore previous policy",
                    true,
                    move || {
                        if let Some(ui) = weak_action.upgrade() {
                            let result = ui.model.borrow_mut().restore_previous();
                            match result {
                                Ok(()) => {
                                    ui.success("Previous policy restored. Restart a running provider to activate it.");
                                    ui.refresh();
                                }
                                Err(error) => ui.error(&error),
                            }
                        }
                    },
                );
            }
        });
    }

    fn refresh(&self) {
        self.updating.set(true);
        let model = self.model.borrow();
        let draft = model.draft();
        self.controls.enabled.set_active(draft.is_enabled());
        for control in &self.controls.capabilities {
            control
                .button
                .set_active(draft.capabilities().contains(&control.capability));
        }
        self.controls
            .clear_grant
            .set_sensitive(draft.grant_uid().is_some());
        let (scope, titles) = draft.observation();
        self.controls.scope.set_selected(match scope {
            ClientScope::None => 0,
            ClientScope::CurrentWorkspace => 1,
            ClientScope::AllWorkspaces => 2,
        });
        self.controls.titles.set_active(titles);
        self.controls
            .launch_mode
            .set_selected(match draft.launch_mode() {
                LaunchMode::Deny => 0,
                LaunchMode::AllowListed => 1,
                LaunchMode::AllowInstalled => 2,
            });
        self.controls
            .user_entries
            .set_active(draft.allows_user_entries());
        for control in &self.controls.catalog {
            let admitted = application_admitted(draft, &control.application);
            control.button.set_active(admitted);
            control.button.set_sensitive(
                draft.launch_mode() != LaunchMode::Deny
                    && (!control.application.user_entry || draft.allows_user_entries()),
            );
        }
        let (sessions, requests, timeout) = draft.resource_limits();
        self.controls.max_sessions.set_value(f64::from(sessions));
        self.controls.max_requests.set_value(f64::from(requests));
        self.controls.io_timeout.set_value(f64::from(timeout));

        self.controls.saved_state.set_text(&format!(
            "Valid and {}",
            if model.saved_enabled() {
                "enabled"
            } else {
                "disabled"
            }
        ));
        self.controls.saved_state.remove_css_class("state-warning");
        self.controls.saved_state.add_css_class("state-good");
        if model.has_changes() {
            self.controls
                .draft_state
                .set_text("Changed · review before saving");
            self.controls.draft_state.remove_css_class("state-good");
            self.controls.draft_state.add_css_class("state-warning");
        } else {
            self.controls.draft_state.set_text("Matches saved policy");
            self.controls.draft_state.remove_css_class("state-warning");
            self.controls.draft_state.add_css_class("state-good");
        }
        self.refresh_active_state(&model);
        match model.unified_diff() {
            Ok(diff) => {
                self.controls.diff.buffer().set_text(&diff);
                self.controls
                    .validation
                    .set_text("Exact candidate validation: valid");
                self.controls.save.set_sensitive(model.has_changes());
            }
            Err(error) => {
                self.controls
                    .diff
                    .buffer()
                    .set_text("Draft cannot be rendered.");
                self.controls
                    .validation
                    .set_text(&format!("Exact candidate validation: {error}"));
                self.controls.save.set_sensitive(false);
            }
        }
        self.controls.discard.set_sensitive(model.has_changes());
        self.controls
            .restore
            .set_sensitive(model.recovery_path().exists());
        drop(model);
        self.updating.set(false);
        self.filter_catalog(&self.controls.catalog_search.text());
    }

    fn refresh_active_state(&self, model: &SettingsModel) {
        self.controls.active_state.remove_css_class("state-good");
        self.controls.active_state.remove_css_class("state-warning");
        self.controls.active_state.remove_css_class("state-unknown");
        let (text, class) = match model.active_policy_status() {
            Ok(ActivePolicyStatus::Matching { pid }) => (
                format!("Matches saved · provider process {pid}"),
                "state-good",
            ),
            Ok(ActivePolicyStatus::Different { pid }) => (
                format!("Different · restart provider process {pid}"),
                "state-warning",
            ),
            Ok(ActivePolicyStatus::Multiple { count, all_match }) if all_match => {
                (format!("{count} providers · all match saved"), "state-good")
            }
            Ok(ActivePolicyStatus::Multiple { count, .. }) => (
                format!("{count} providers · at least one needs restart"),
                "state-warning",
            ),
            Ok(ActivePolicyStatus::NotReported) => (
                "Not reported · provider stopped or older".to_owned(),
                "state-unknown",
            ),
            Ok(ActivePolicyStatus::Unavailable) => (
                "Unavailable · XDG runtime directory is not set".to_owned(),
                "state-unknown",
            ),
            Err(error) => (format!("Unavailable · {error}"), "state-warning"),
        };
        self.controls.active_state.set_text(&text);
        self.controls.active_state.add_css_class(class);
    }

    fn refresh_runtime_seat(&self) {
        self.show_runtime_seat(control_runtime_seat(RuntimeSeatCommand::Status));
    }

    fn change_runtime_seat(&self, command: RuntimeSeatCommand) {
        match control_runtime_seat(command) {
            Ok(status) => {
                self.show_runtime_seat(Ok(status));
                if status.is_enabled() {
                    self.success(
                        "Runtime seat enabled for this provider instance. Saved policy did not change.",
                    );
                } else {
                    self.success(
                        "Runtime seat disabled. Current sessions are revoked and saved policy did not change.",
                    );
                }
            }
            Err(error) => {
                self.show_runtime_seat(Err(error.clone()));
                self.error(&format!("Runtime seat was not changed: {error}"));
            }
        }
    }

    fn show_runtime_seat(&self, result: Result<RuntimeSeatStatus, String>) {
        let presentation = runtime_seat_presentation(&result);
        for class in [
            "state-good",
            "state-warning",
            "state-unknown",
            "state-enabled",
            "state-disabled",
        ] {
            self.controls.runtime.state.remove_css_class(class);
        }
        self.controls.runtime.state.set_text(&presentation.rail);
        self.controls
            .runtime
            .state
            .add_css_class(presentation.class);
        self.controls.runtime.detail.set_text(&presentation.detail);
        self.controls
            .runtime
            .enable
            .set_sensitive(presentation.can_enable);
        self.controls
            .runtime
            .disable
            .set_sensitive(presentation.can_disable);
    }

    fn apply(
        &self,
        operation: impl FnOnce(&mut agent_seat_x11::PolicyDraft) -> Result<(), String>,
    ) {
        if self.updating.get() {
            return;
        }
        let result = self.model.borrow_mut().edit(operation);
        match result {
            Ok(()) => self.clear_message(),
            Err(error) => self.error(&error),
        }
        self.refresh();
    }

    fn edit_limits(&self) {
        if self.updating.get() {
            return;
        }
        let sessions = u8::try_from(self.controls.max_sessions.value_as_int());
        let requests = u16::try_from(self.controls.max_requests.value_as_int());
        let timeout = u32::try_from(self.controls.io_timeout.value_as_int());
        match (sessions, requests, timeout) {
            (Ok(sessions), Ok(requests), Ok(timeout)) => {
                self.apply(move |draft| draft.set_resource_limits(sessions, requests, timeout));
            }
            _ => self.error("A displayed resource limit cannot be represented safely."),
        }
    }

    fn change_launch_mode(&self, mode: LaunchMode) {
        if self.updating.get() {
            return;
        }
        let result = self
            .model
            .borrow_mut()
            .edit(|draft| draft.set_launch_mode(mode));
        match result {
            Ok(()) => self.clear_message(),
            Err(error) => self.error(&error),
        }
        self.refresh();
    }

    fn filter_catalog(&self, query: &str) {
        let query = query.trim().to_lowercase();
        let mut visible = 0_usize;
        for control in &self.controls.catalog {
            let show = query.is_empty() || control.search_key.contains(&query);
            control.row.set_visible(show);
            if show {
                visible += 1;
            }
        }
        self.controls.catalog_count.set_text(&format!(
            "Showing {visible} of {} bounded launchable entries",
            self.controls.catalog.len()
        ));
        self.controls
            .catalog_error
            .set_visible(!self.controls.catalog_error.text().is_empty());
    }

    fn confirm_save(self: &Rc<Self>) {
        if !self.model.borrow().has_changes() {
            return;
        }
        let active_guidance = match self.model.borrow().active_policy_status() {
            Ok(
                ActivePolicyStatus::Matching { .. }
                | ActivePolicyStatus::Different { .. }
                | ActivePolicyStatus::Multiple { .. },
            ) => "A running provider will keep its current policy until you restart it.",
            Ok(ActivePolicyStatus::NotReported) => {
                "No active policy was reported. Saving will not start the provider."
            }
            Ok(ActivePolicyStatus::Unavailable) | Err(_) => {
                "Active policy state is unavailable. If a provider is running, restart it after saving."
            }
        };
        let weak = Rc::downgrade(self);
        self.confirm(
            "Save the reviewed policy?",
            active_guidance,
            "Save reviewed changes",
            false,
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.save_now();
                }
            },
        );
    }

    fn save_now(&self) {
        let result = self.model.borrow_mut().save();
        match result {
            Ok(()) => {
                self.refresh();
                let message = match self.model.borrow().active_policy_status() {
                    Ok(
                        ActivePolicyStatus::Different { .. }
                        | ActivePolicyStatus::Multiple {
                            all_match: false, ..
                        },
                    ) => "Changes saved. Restart the running provider to activate them.",
                    Ok(ActivePolicyStatus::Matching { .. }) => {
                        "Changes saved and the reported active policy matches."
                    }
                    Ok(ActivePolicyStatus::Multiple {
                        all_match: true, ..
                    }) => "Changes saved and all reported active policies match.",
                    Ok(ActivePolicyStatus::NotReported) => {
                        "Changes saved. Start the provider when you are ready."
                    }
                    Ok(ActivePolicyStatus::Unavailable) | Err(_) => {
                        "Changes saved. Restart any running provider to activate them."
                    }
                };
                self.success(message);
            }
            Err(error) => {
                self.error(&error);
                self.refresh();
            }
        }
    }

    fn reload(self: &Rc<Self>) {
        if self.model.borrow().has_changes() {
            let weak = Rc::downgrade(self);
            self.confirm(
                "Reload and discard the draft?",
                "The policy will be read again from disk. Unsaved controls will be discarded.",
                "Reload saved policy",
                true,
                move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.reload_now();
                    }
                },
            );
        } else {
            self.reload_now();
        }
    }

    fn reload_now(&self) {
        let result = self.model.borrow_mut().reload();
        match result {
            Ok(()) => {
                self.success("Saved policy reloaded.");
                self.refresh();
            }
            Err(error) => self.error(&error),
        }
    }

    fn close_request(self: &Rc<Self>) -> glib::Propagation {
        if self.allow_close.get() || !self.model.borrow().has_changes() {
            return glib::Propagation::Proceed;
        }
        let weak = Rc::downgrade(self);
        self.confirm(
            "Discard draft and close?",
            "The saved policy will not change.",
            "Discard and close",
            true,
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.allow_close.set(true);
                    if let Some(window) = ui.window.upgrade() {
                        window.close();
                    }
                }
            },
        );
        glib::Propagation::Stop
    }

    fn confirm(
        &self,
        title: &str,
        body: &str,
        action_label: &str,
        destructive: bool,
        action: impl Fn() + 'static,
    ) {
        let Some(parent) = self.window.upgrade() else {
            return;
        };
        let dialog = gtk::Window::builder()
            .title(title)
            .transient_for(&parent)
            .modal(true)
            .resizable(false)
            .default_width(460)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.set_margin_top(24);
        content.set_margin_end(24);
        content.set_margin_bottom(24);
        content.set_margin_start(24);
        content.append(&label(title, "page-title"));
        let explanation = label(body, "control-description");
        explanation.set_wrap(true);
        content.append(&explanation);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let confirm = gtk::Button::with_label(action_label);
        confirm.add_css_class(if destructive {
            "destructive-action"
        } else {
            "suggested-action"
        });
        let weak_dialog = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
        let weak_dialog = dialog.downgrade();
        confirm.connect_clicked(move |_| {
            action();
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
        buttons.append(&cancel);
        buttons.append(&confirm);
        content.append(&buttons);
        dialog.set_child(Some(&content));
        dialog.present();
    }

    fn clear_message(&self) {
        self.controls.message.set_visible(false);
        self.controls.message.set_text("");
        self.controls.message.remove_css_class("message-error");
        self.controls.message.remove_css_class("message-success");
    }

    fn error(&self, message: &str) {
        self.controls.message.remove_css_class("message-success");
        self.controls.message.add_css_class("message-error");
        self.controls.message.set_text(message);
        self.controls.message.set_visible(true);
    }

    fn success(&self, message: &str) {
        self.controls.message.remove_css_class("message-error");
        self.controls.message.add_css_class("message-success");
        self.controls.message.set_text(message);
        self.controls.message.set_visible(true);
    }
}

fn apply_weak(
    weak: &Weak<Ui>,
    operation: impl FnOnce(&mut agent_seat_x11::PolicyDraft) -> Result<(), String>,
) {
    if let Some(ui) = weak.upgrade() {
        ui.apply(operation);
    }
}

fn edit_application(
    draft: &mut agent_seat_x11::PolicyDraft,
    id: ApplicationId,
    admitted: bool,
) -> Result<(), String> {
    let mut allow = draft.launch_allow().to_vec();
    let mut deny = draft.launch_deny().to_vec();
    match draft.launch_mode() {
        LaunchMode::Deny => return Err("Choose an allowing launch mode first.".to_owned()),
        LaunchMode::AllowListed => {
            if admitted && !allow.contains(&id) {
                deny.retain(|entry| *entry != id);
                allow.push(id);
            } else if !admitted {
                allow.retain(|entry| *entry != id);
            }
        }
        LaunchMode::AllowInstalled => {
            if admitted {
                deny.retain(|entry| *entry != id);
            } else if !deny.contains(&id) {
                allow.retain(|entry| *entry != id);
                deny.push(id);
            }
        }
    }
    draft.set_launch(
        draft.launch_mode(),
        allow,
        deny,
        draft.allows_user_entries(),
    )
}

fn application_admitted(
    draft: &agent_seat_x11::PolicyDraft,
    application: &ApplicationDescriptor,
) -> bool {
    if application.user_entry && !draft.allows_user_entries() {
        return false;
    }
    match draft.launch_mode() {
        LaunchMode::Deny => false,
        LaunchMode::AllowListed => draft.launch_allow().contains(&application.id),
        LaunchMode::AllowInstalled => !draft.launch_deny().contains(&application.id),
    }
}

struct CapabilitySpec {
    capability: Capability,
    title: &'static str,
    atom: &'static str,
    description: &'static str,
    dependency: &'static str,
}

fn capability_groups() -> [(&'static str, &'static str, Vec<CapabilitySpec>); 5] {
    [
        (
            "Observe",
            "Read bounded desktop state.",
            vec![
                CapabilitySpec {
                    capability: Capability::ObserveStructure,
                    title: "Window structure",
                    atom: "observe_structure",
                    description: "Read workspaces, client identities, geometry, and public state.",
                    dependency: "",
                },
                CapabilitySpec {
                    capability: Capability::ObserveTitles,
                    title: "Window titles",
                    atom: "observe_titles",
                    description: "Read title text when the independent title switch also allows it.",
                    dependency: "Window structure",
                },
                CapabilitySpec {
                    capability: Capability::ObserveEvents,
                    title: "Desktop changes",
                    atom: "observe_events",
                    description: "Poll the bounded stream of visible desktop changes.",
                    dependency: "Window structure",
                },
            ],
        ),
        (
            "Manage",
            "Send supported EWMH requests and observe their public outcome.",
            vec![
                CapabilitySpec {
                    capability: Capability::ManageActivate,
                    title: "Activate windows",
                    atom: "manage_activate",
                    description: "Ask the window manager to focus a visible client.",
                    dependency: "Window structure",
                },
                CapabilitySpec {
                    capability: Capability::ManageClose,
                    title: "Close windows",
                    atom: "manage_close",
                    description: "Ask a client to close politely.",
                    dependency: "Window structure",
                },
                CapabilitySpec {
                    capability: Capability::ManageWorkspace,
                    title: "Change workspaces",
                    atom: "manage_workspace",
                    description: "Switch workspaces or move a visible client between them.",
                    dependency: "Window structure",
                },
                CapabilitySpec {
                    capability: Capability::ManageState,
                    title: "Change window state",
                    atom: "manage_state",
                    description: "Change states that the client and window manager advertise.",
                    dependency: "Window structure",
                },
                CapabilitySpec {
                    capability: Capability::ManageGeometry,
                    title: "Move and resize windows",
                    atom: "manage_geometry",
                    description: "Request decoration-aware frame geometry changes.",
                    dependency: "Window structure",
                },
            ],
        ),
        (
            "Launch",
            "Discover and start policy-approved desktop entries without a shell.",
            vec![
                CapabilitySpec {
                    capability: Capability::LaunchList,
                    title: "List applications",
                    atom: "launch_list",
                    description: "List applications admitted by the launch policy.",
                    dependency: "",
                },
                CapabilitySpec {
                    capability: Capability::LaunchExecute,
                    title: "Launch applications",
                    atom: "launch_execute",
                    description: "Start an admitted desktop entry.",
                    dependency: "List applications",
                },
            ],
        ),
        (
            "Input",
            "Use X11 input only while the current provider seat is enabled.",
            vec![
                CapabilitySpec {
                    capability: Capability::InputPointer,
                    title: "Pointer movement and clicks",
                    atom: "input_pointer",
                    description: "Move or click only at a currently visible point inside the target client.",
                    dependency: "Window structure and an enabled runtime seat",
                },
                CapabilitySpec {
                    capability: Capability::InputKeyboard,
                    title: "Keyboard text",
                    atom: "input_keyboard",
                    description: "Type bounded text or send one key command only when the target already owns keyboard focus.",
                    dependency: "Window structure and an enabled runtime seat",
                },
            ],
        ),
        (
            "Capture",
            "Read bounded pixels owned by one freshly observed client.",
            vec![CapabilitySpec {
                capability: Capability::CaptureObscured,
                title: "Obscured client pixels",
                atom: "capture_obscured",
                description: "Capture the target client's own pixels even when another window covers it.",
                dependency: "Window structure",
            }],
        ),
    ]
}

fn page(title: &str, introduction: &str) -> (gtk::Box, gtk::ScrolledWindow) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("page");
    content.append(&label(title, "page-title"));
    let intro = label(introduction, "page-intro");
    intro.set_wrap(true);
    content.append(&intro);
    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();
    (content, scroller)
}

fn panel(title: &str, description: &str) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 3);
    panel.add_css_class("panel");
    panel.append(&label(title, "panel-title"));
    let description = label(description, "panel-description");
    description.set_wrap(true);
    panel.append(&description);
    panel
}

fn control_row(title: &str, description: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.add_css_class("control-row");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(title, "control-title"));
    let description = label(description, "control-description");
    description.set_wrap(true);
    text.append(&description);
    row.append(&text);
    row.append(control);
    row
}

fn value_row(title: &str, value: &str) -> gtk::Box {
    let value = label(value, "atom");
    value.set_selectable(true);
    value.set_wrap(true);
    control_row(title, "", &value)
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).xalign(0.0).build();
    label.add_css_class(class);
    label
}

fn build_error_window(application: &Application, error: &str) {
    install_style();
    let window = ApplicationWindow::builder()
        .application(application)
        .title("Agent Seat Settings")
        .default_width(620)
        .default_height(280)
        .build();
    window.add_css_class("agent-seat-window");
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.add_css_class("page");
    page.append(&label("Policy could not be opened", "page-title"));
    let detail = label(error, "control-description");
    detail.set_wrap(true);
    detail.set_selectable(true);
    page.append(&detail);
    page.append(&label(
        "Use agent-seat-settings --check in a terminal for the same strict error without a display.",
        "control-description",
    ));
    let close = gtk::Button::with_label("Close");
    close.set_halign(gtk::Align::End);
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

#[cfg(test)]
mod tests {
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};

    use agent_seat_x11::read_policy;

    use super::*;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn application_selection_keeps_allow_and_deny_lists_consistent() {
        let directory =
            std::env::temp_dir().join(format!("agent-seat-settings-ui-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(&directory).expect("UI fixture");
        let fixture = Fixture(directory);
        let path = fixture.0.join("config.toml");
        fs::write(
            &path,
            "enabled = false\n[launch]\nmode = \"allow_listed\"\ndeny = [\"blocked.desktop\"]\n",
        )
        .expect("write UI policy");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("secure UI policy");
        let mut draft = read_policy(&path).expect("read UI policy").draft();
        let blocked = ApplicationId::new("blocked.desktop").expect("blocked desktop ID");

        edit_application(&mut draft, blocked.clone(), true).expect("admit blocked application");

        assert_eq!(draft.launch_allow(), std::slice::from_ref(&blocked));
        assert!(draft.launch_deny().is_empty());

        draft
            .set_launch_mode(LaunchMode::AllowInstalled)
            .expect("switch UI policy to installed mode");
        edit_application(&mut draft, blocked.clone(), false).expect("deny installed application");
        assert!(draft.launch_allow().is_empty());
        assert_eq!(draft.launch_deny(), std::slice::from_ref(&blocked));
    }

    #[test]
    fn runtime_seat_presentation_keeps_volatile_actions_explicit() {
        let disabled = known_runtime_seat_presentation(false, 7);
        assert_eq!(disabled.rail, "Disabled · generation 7");
        assert!(
            disabled
                .detail
                .contains("New Agent Seat sessions are denied")
        );
        assert!(disabled.can_enable);
        assert!(!disabled.can_disable);

        let enabled = known_runtime_seat_presentation(true, 7);
        assert_eq!(enabled.rail, "Enabled · generation 7");
        assert!(enabled.detail.contains("current provider instance"));
        assert!(!enabled.can_enable);
        assert!(enabled.can_disable);

        let unavailable = runtime_seat_presentation(&Err("no provider".to_owned()));
        assert_eq!(unavailable.rail, "Unavailable · denied");
        assert!(!unavailable.can_enable);
        assert!(!unavailable.can_disable);
    }

    #[test]
    fn access_page_exposes_separate_pointer_and_keyboard_grants() {
        let groups = capability_groups();
        let input = groups
            .iter()
            .find(|(name, _, _)| *name == "Input")
            .expect("Input capability group");
        assert_eq!(input.2.len(), 2);
        assert!(
            input
                .2
                .iter()
                .any(|spec| spec.capability == Capability::InputPointer)
        );
        assert!(
            input
                .2
                .iter()
                .any(|spec| spec.capability == Capability::InputKeyboard)
        );
        assert!(
            input
                .2
                .iter()
                .all(|spec| spec.dependency.contains("enabled runtime seat"))
        );
    }

    #[test]
    fn access_page_exposes_obscured_capture_as_a_separate_grant() {
        let groups = capability_groups();
        let capture = groups
            .iter()
            .find(|(name, _, _)| *name == "Capture")
            .expect("Capture capability group");
        assert_eq!(capture.2.len(), 1);
        assert_eq!(capture.2[0].capability, Capability::CaptureObscured);
        assert_eq!(capture.2[0].dependency, "Window structure");
    }
}
