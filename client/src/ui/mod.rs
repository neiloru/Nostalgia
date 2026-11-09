//! The window: one view that owns the UI state, renders the pages and handles all input.

pub mod assets;
pub mod gamepad;
pub mod nav;
mod pages;
mod shell;
pub mod theme;
pub mod widgets;

use crate::client::{ApiError, Client, UiEvent};
use crate::model::{ClientState, DownloadJob};
use futures_util::StreamExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nav::{ActionFn, Dir, InputMode, Nav, NavRc};
use nostalgia_shared::{GameDetail, ItemKind, SaveVersion};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    Vault,
    Library,
    Downloads,
    Settings,
}

pub const TABS: [(Tab, &str); 4] = [(Tab::Vault, "Vault"), (Tab::Library, "Library"), (Tab::Downloads, "Downloads"), (Tab::Settings, "Settings")];

pub struct Toast {
    pub id: u64,
    pub message: String,
    pub error: bool,
}

pub struct SelectOption {
    pub label: String,
    pub hint: Option<String>,
}

pub type PickFn = Rc<dyn Fn(&mut AppView, usize, &mut Window, &mut Context<AppView>)>;

pub enum Modal {
    Select { title: String, options: Vec<SelectOption>, selected: usize, on_pick: PickFn },
    Confirm { title: String, message: String, confirm: String, danger: bool, on_confirm: ActionFn },
    InstallTo { kind: ItemKind, id: i64 },
    Lightbox { game_id: i64, shots: Vec<String>, index: usize },
    SaveVersions { emulator_id: i64, versions: Option<Vec<SaveVersion>> },
    /// Vault filter with several values: options toggle and the dialog stays open. `values[0]`
    /// is "" (all), the rest are the values of `Filters::list(key)`.
    MultiSelect { title: String, key: &'static str, values: Vec<String>, labels: Vec<String> },
}

/// Vault filters (persisted like the Electron client's localStorage). Each list filter matches any
/// of its values; an empty list is "all".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    #[serde(deserialize_with = "one_or_many")]
    pub platform: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub genre: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub theme: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub mode: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub developer: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub decade: Vec<String>,
    pub sort: String,
    pub show_emulators: bool,
    pub installed_only: bool,
    pub library_only: bool,
}

impl Filters {
    pub const LISTS: [&'static str; 6] = ["platform", "genre", "theme", "mode", "decade", "developer"];

    pub fn list(&self, key: &str) -> &Vec<String> {
        match key {
            "platform" => &self.platform,
            "genre" => &self.genre,
            "theme" => &self.theme,
            "mode" => &self.mode,
            "decade" => &self.decade,
            _ => &self.developer,
        }
    }

    pub fn list_mut(&mut self, key: &str) -> &mut Vec<String> {
        match key {
            "platform" => &mut self.platform,
            "genre" => &mut self.genre,
            "theme" => &mut self.theme,
            "mode" => &mut self.mode,
            "decade" => &mut self.decade,
            _ => &mut self.developer,
        }
    }

    /// Adds or removes a value; "" clears the filter.
    pub fn toggle(&mut self, key: &str, value: &str) {
        let list = self.list_mut(key);
        if value.is_empty() {
            list.clear();
        } else if let Some(i) = list.iter().position(|v| v == value) {
            list.remove(i);
        } else {
            list.push(value.to_string());
        }
    }
}

/// Filters saved by older versions hold one value as a string ("" = all).
fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
        Null(()),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) if s.is_empty() => Vec::new(),
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v.into_iter().filter(|s| !s.is_empty()).collect(),
        OneOrMany::Null(()) => Vec::new(),
    })
}

/// Small per-device UI preferences (ui.json next to the client data).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiPrefs {
    pub vault_filters: Filters,
    pub lib_emulators: bool,
    pub lib_installed: bool,
    pub last_server: Option<String>,
    pub last_user: Option<String>,
}

pub struct LocInfo {
    pub path: String,
    pub free: Option<u64>,
    pub exists: bool,
}

pub struct Inputs {
    pub server: Entity<InputState>,
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
    pub password2: Entity<InputState>,
    pub vault_search: Entity<InputState>,
    pub lib_search: Entity<InputState>,
    pub extra_args: Entity<InputState>,
    pub old_pw: Entity<InputState>,
    pub new_pw: Entity<InputState>,
    pub new_pw2: Entity<InputState>,
}

pub struct AppView {
    pub client: Arc<Client>,
    pub state: Arc<ClientState>,
    pub jobs: Vec<DownloadJob>,
    pub nav: NavRc,
    pub focus_handle: FocusHandle,
    pub tab: Tab,
    pub toasts: Vec<Toast>,
    toast_seq: u64,
    pub modals: Vec<(Modal, Option<SharedString>)>,
    pub inputs: Inputs,
    pub prefs: UiPrefs,
    // login
    pub register: bool,
    pub login_busy: bool,
    pub login_error: String,
    // vault / store
    pub store_sel: Option<(ItemKind, i64)>,
    pub last_card: Option<String>,
    // library
    pub lib_sel: Option<(ItemKind, i64)>,
    pub args_for: Option<i64>,
    // settings
    pub pw_error: String,
    pub locations: Vec<LocInfo>,
    // data loaded on demand
    pub details: HashMap<i64, Option<GameDetail>>,
    pub details_loading: HashSet<i64>,
    // scroll areas
    pub scrolls: HashMap<&'static str, ScrollHandle>,
    pending_focus: Option<FocusTarget>,
    /// Controller family last used (button prompts).
    pub pad_kind: gamepad::PadKind,
    /// Frames a pending focus has waited for its target to appear.
    pending_focus_tries: u8,
    _subs: Vec<Subscription>,
}

#[derive(Clone, Debug)]
pub enum FocusTarget {
    FirstIn(&'static str),
    Group(&'static str),
    Id(String),
    First,
}

fn prefs_path(client: &Client) -> std::path::PathBuf {
    client.paths.user_data.join("ui.json")
}

impl AppView {
    pub fn new(client: Arc<Client>, ui_rx: futures_channel::mpsc::UnboundedReceiver<UiEvent>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs: UiPrefs = std::fs::read_to_string(prefs_path(&client)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mk = |placeholder: &str, masked: bool, value: &str, window: &mut Window, cx: &mut Context<Self>| {
            let p = placeholder.to_string();
            let v = value.to_string();
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(p).masked(masked);
                if !v.is_empty() {
                    s.set_value(v, window, cx);
                }
                s
            })
        };
        let inputs = Inputs {
            server: mk("http://192.168.1.10:8080", false, prefs.last_server.as_deref().unwrap_or("http://localhost:8080"), window, cx),
            username: mk("", false, prefs.last_user.as_deref().unwrap_or(""), window, cx),
            password: mk("", true, "", window, cx),
            password2: mk("", true, "", window, cx),
            vault_search: mk("Search the vault…", false, "", window, cx),
            lib_search: mk("Search library…", false, "", window, cx),
            extra_args: mk("optional", false, "", window, cx),
            old_pw: mk("", true, "", window, cx),
            new_pw: mk("", true, "", window, cx),
            new_pw2: mk("", true, "", window, cx),
        };
        let mut subs = Vec::new();
        let named = [
            (&inputs.server, "login:server"),
            (&inputs.username, "login:username"),
            (&inputs.password, "login:password"),
            (&inputs.password2, "login:password2"),
            (&inputs.vault_search, "vault:search"),
            (&inputs.lib_search, "lib:search"),
            (&inputs.extra_args, "lib:args"),
            (&inputs.old_pw, "settings:oldpw"),
            (&inputs.new_pw, "settings:newpw"),
            (&inputs.new_pw2, "settings:newpw2"),
        ];
        for (input, id) in named {
            subs.push(cx.subscribe_in(input, window, move |this: &mut AppView, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Focus => {
                    this.nav.borrow_mut().set_focus_id(id);
                    cx.notify();
                }
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    if id.starts_with("login:") {
                        this.submit_login(window, cx);
                    }
                }
                InputEvent::Blur => {
                    if id == "lib:args" {
                        this.save_extra_args(cx);
                    }
                }
            }));
        }
        subs.push(cx.observe_window_bounds(window, |this, window, cx| this.apply_zoom(window, cx)));

        // Client events -> UI.
        cx.spawn_in(window, async move |this, cx| {
            let mut rx = ui_rx;
            while let Some(ev) = rx.next().await {
                if this.update_in(cx, |this, window, cx| this.on_client_event(ev, window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        // Controller input.
        let pad_rx = gamepad::start();
        cx.spawn_in(window, async move |this, cx| {
            let mut rx = pad_rx;
            while let Some((cmd, kind)) = rx.next().await {
                if this.update_in(cx, |this, window, cx| this.on_pad(cmd, kind, window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        if std::env::var("NOSTALGIA_DEBUG").is_ok() {
            eprintln!("[ui] scale {} viewport {:?} bounds {:?} rem {:?}", window.scale_factor(), window.viewport_size(), window.bounds(), window.rem_size());
        }
        let state = client.state();
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut view = AppView {
            jobs: client.download_jobs(),
            state,
            client,
            nav: Nav::new(),
            focus_handle,
            tab: Tab::Vault,
            toasts: Vec::new(),
            toast_seq: 0,
            modals: Vec::new(),
            inputs,
            prefs,
            register: false,
            login_busy: false,
            login_error: String::new(),
            store_sel: None,
            last_card: None,
            lib_sel: None,
            args_for: None,
            pw_error: String::new(),
            locations: Vec::new(),
            details: HashMap::new(),
            details_loading: HashSet::new(),
            scrolls: HashMap::new(),
            pending_focus: None,
            pad_kind: gamepad::PadKind::default_for_system(),
            pending_focus_tries: 0,
            _subs: subs,
        };
        view.apply_zoom(window, cx);
        view.request_focus(FocusTarget::First, window, cx);
        view
    }

    pub fn scroll(&mut self, key: &'static str) -> ScrollHandle {
        self.scrolls.entry(key).or_default().clone()
    }

    pub fn save_prefs(&self) {
        let _ = crate::store::write_atomic(&prefs_path(&self.client), &self.prefs);
    }

    // ------------------------------------------------------------------ client events
    fn on_client_event(&mut self, ev: UiEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            UiEvent::State => {
                let was_logged_in = self.state.session.is_some();
                self.state = self.client.state();
                if was_logged_in != self.state.session.is_some() {
                    self.modals.clear();
                    self.request_focus(FocusTarget::First, window, cx);
                }
            }
            UiEvent::Downloads => self.jobs = self.client.download_jobs(),
            UiEvent::Toast { message, error } => self.toast(message, error, cx),
            UiEvent::Minimize => window.minimize_window(),
            UiEvent::Restore => window.activate_window(),
        }
        cx.notify();
    }

    pub fn toast(&mut self, message: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.toast_seq += 1;
        let id = self.toast_seq;
        let message: String = message.into();
        if self.toasts.len() >= 4 {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { id, message, error });
        let ms = if error { 7000 } else { 4000 };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
            let _ = this.update(cx, |this, cx| {
                this.toasts.retain(|t| t.id != id);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Run a client operation on the tokio runtime; errors become toasts.
    pub fn act<T: Send + 'static>(
        &mut self,
        fut: impl std::future::Future<Output = Result<T, ApiError>> + Send + 'static,
        success: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let task = self.client.rt.spawn(fut);
        cx.spawn(async move |this, cx| {
            let r = task.await;
            let _ = this.update(cx, |this, cx| match r {
                Ok(Ok(_)) => {
                    if let Some(s) = success {
                        this.toast(s, false, cx);
                    }
                }
                Ok(Err(e)) => this.toast(e.message, true, cx),
                Err(e) => this.toast(e.to_string(), true, cx),
            });
        })
        .detach();
    }

    // ------------------------------------------------------------------ zoom
    /// The UI is laid out for 1920x1080; bigger windows are zoomed (1.0 … 3.0 in 5 % steps).
    pub fn apply_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = window.viewport_size();
        let scale = self.client.settings().ui_scale;
        let zoom = if scale > 0.0 {
            scale
        } else {
            let z = (f32::from(size.width) / 1920.0).min(f32::from(size.height) / 1080.0);
            ((z.clamp(1.0, 3.0) * 20.0).round() / 20.0) as f64
        };
        let rem = px(16.0 * zoom as f32);
        if window.rem_size() != rem {
            window.set_rem_size(rem);
            gpui_kit::component::theme::Theme::global_mut(cx).font_size = px(15.0 * zoom as f32);
        }
        self.client.set_window_state(window.is_fullscreen(), zoom);
        cx.notify();
    }

    pub fn step_ui_scale(&mut self, dir: i32, window: &mut Window, cx: &mut Context<Self>) {
        const STEPS: [f64; 10] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
        let s = self.client.settings();
        let cur = if s.ui_scale > 0.0 { s.ui_scale } else { self.state.ui_zoom };
        let next = match dir {
            0 => Some(0.0),
            d if d > 0 => STEPS.iter().copied().find(|x| *x > cur + 0.001),
            _ => STEPS.iter().rev().copied().find(|x| *x < cur - 0.001),
        };
        if let Some(n) = next {
            self.client.update_settings(|s| s.ui_scale = n);
            self.apply_zoom(window, cx);
        }
    }

    pub fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.toggle_fullscreen();
        let me = cx.entity().downgrade();
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let _ = me.update_in(cx, |this, window, cx| this.apply_zoom(window, cx));
        })
        .detach();
    }

    // ------------------------------------------------------------------ focus
    pub fn request_focus(&mut self, target: FocusTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_focus = Some(target);
        self.pending_focus_tries = 0;
        // `on_next_frame` runs before that frame is rendered, i.e. with the bounds of the previous
        // frame. Wait one more frame so the changed UI (new page, closed dialog) is laid out.
        cx.on_next_frame(window, |_, window, cx| {
            cx.on_next_frame(window, |this, window, cx| this.apply_pending_focus(window, cx));
            cx.notify();
        });
        cx.notify();
    }

    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.pending_focus.take() else { return };
        let idx = {
            let n = self.nav.borrow();
            match &target {
                FocusTarget::First => n.first_index(None),
                FocusTarget::FirstIn(g) => n.first_index(Some(g)),
                FocusTarget::Group(g) => n.group_entry(g),
                FocusTarget::Id(id) => n.index_of(id).or_else(|| n.recover_index()).or_else(|| n.first_index(None)),
            }
        };
        match idx {
            Some(i) => self.focus_index(i, window, cx),
            // The target is not laid out yet (a page that is still being built): keep the request
            // and try again on the next frames instead of leaving the focus on a stale item.
            None if self.pending_focus_tries < 30 => {
                self.pending_focus = Some(target);
                self.pending_focus_tries += 1;
                cx.on_next_frame(window, |this, window, cx| this.apply_pending_focus(window, cx));
            }
            None => {}
        }
        cx.notify();
    }

    fn focus_index(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let input = {
            let mut n = self.nav.borrow_mut();
            n.set_focus_index(i);
            n.focused_item().and_then(|it| it.input.clone())
        };
        self.sync_input_focus(input, window, cx);
    }

    pub fn focus_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let idx = self.nav.borrow().index_of(id);
        if let Some(i) = idx {
            self.focus_index(i, window, cx);
        }
    }

    /// Text fields get real keyboard focus while they are the navigation focus, nothing else does.
    fn sync_input_focus(&mut self, input: Option<Entity<InputState>>, window: &mut Window, cx: &mut Context<Self>) {
        match input {
            Some(i) => {
                if self.nav.borrow().mode != InputMode::Mouse || !i.read(cx).focus_handle(cx).is_focused(window) {
                    i.update(cx, |s, cx| s.focus(window, cx));
                }
            }
            None => self.focus_handle.focus(window, cx),
        }
    }

    fn text_focused(&self, window: &Window, cx: &App) -> Option<Entity<InputState>> {
        let n = self.nav.borrow();
        let input = n.focused_item().and_then(|it| it.input.clone())?;
        input.read(cx).focus_handle(cx).is_focused(window).then_some(input)
    }

    pub fn set_mode(&mut self, m: InputMode, cx: &mut Context<Self>) {
        if self.nav.borrow().mode != m {
            self.nav.borrow_mut().mode = m;
            cx.notify();
        }
    }

    pub fn move_focus(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) {
        let target = {
            let n = self.nav.borrow();
            if n.focused_present() { n.find_move(dir) } else { n.recover_index().or_else(|| n.first_index(None)) }
        };
        if let Some(i) = target {
            self.focus_index(i, window, cx);
        }
        cx.notify();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.nav.borrow().focused_present() {
            let i = {
                let n = self.nav.borrow();
                n.recover_index().or_else(|| n.first_index(None))
            };
            if let Some(i) = i {
                self.focus_index(i, window, cx);
            }
            return;
        }
        let (action, input) = {
            let n = self.nav.borrow();
            let it = n.focused_item();
            (it.and_then(|i| i.action.clone()), it.and_then(|i| i.input.clone()))
        };
        if let Some(input) = input {
            input.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(window, cx);
            });
            return;
        }
        if let Some(a) = action {
            a(self, window, cx);
        }
        // If the button replaced itself (e.g. "Add to library"), keep the focus where it was.
        cx.on_next_frame(window, |_, window, cx| {
            cx.on_next_frame(window, |this, window, cx| {
                let missing = !this.nav.borrow().focused_present() && this.pending_focus.is_none();
                if missing && this.nav.borrow().mode != InputMode::Mouse {
                    let i = this.nav.borrow().recover_index();
                    if let Some(i) = i {
                        this.focus_index(i, window, cx);
                        cx.notify();
                    }
                }
            });
            cx.notify();
        });
        cx.notify();
    }

    /// B / Escape: close a dialog, leave the store page, else go to the tab bar.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.modals.is_empty() {
            self.close_modal(window, cx);
        } else if self.state.session.is_some() && self.tab == Tab::Vault && self.store_sel.is_some() {
            self.close_store(window, cx);
        } else if self.state.session.is_some() {
            let in_bar = self.nav.borrow().focused.as_deref().is_some_and(|f| f.starts_with("tab:") || f.starts_with("bar:"));
            if !in_bar {
                self.request_focus(FocusTarget::Group("bar"), window, cx);
            }
        }
        cx.notify();
    }

    pub fn open_modal(&mut self, m: Modal, window: &mut Window, cx: &mut Context<Self>) {
        let prev = self.nav.borrow().focused.clone();
        self.modals.push((m, prev));
        self.request_focus(FocusTarget::FirstIn("modal"), window, cx);
    }

    pub fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, prev)) = self.modals.pop() {
            match prev {
                Some(id) => self.request_focus(FocusTarget::Id(id.to_string()), window, cx),
                None => self.request_focus(FocusTarget::First, window, cx),
            }
        }
        cx.notify();
    }

    pub fn set_tab(&mut self, t: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab != t {
            self.tab = t;
            if t == Tab::Settings {
                self.refresh_locations();
            }
        }
        self.request_focus(FocusTarget::FirstIn("content"), window, cx);
    }

    fn cycle_tab(&mut self, d: i32, window: &mut Window, cx: &mut Context<Self>) {
        if !self.modals.is_empty() || self.state.session.is_none() {
            return;
        }
        let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0) as i32;
        let n = TABS.len() as i32;
        self.set_tab(TABS[((i + d + n) % n) as usize].0, window, cx);
    }

    /// Y: search / filter on the vault and library pages.
    fn secondary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.modals.is_empty() || self.state.session.is_none() {
            return;
        }
        let id = match self.tab {
            Tab::Vault if self.store_sel.is_none() => "vault:search",
            Tab::Library => "lib:search",
            _ => return,
        };
        self.focus_id(id, window, cx);
    }

    // ------------------------------------------------------------------ keyboard / controller
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let key = ks.key.as_str();
        if std::env::var("NOSTALGIA_DEBUG").is_ok() {
            eprintln!("[key] {key:?} focused={:?}", self.nav.borrow().focused);
        }
        let m = &ks.modifiers;
        if (m.control || m.platform) && !m.alt {
            let dir = match key {
                "+" | "=" => Some(1),
                "-" => Some(-1),
                "0" => Some(0),
                _ => None,
            };
            if let Some(d) = dir {
                self.step_ui_scale(d, window, cx);
                cx.stop_propagation();
                return;
            }
        }
        if key == "f11" {
            self.toggle_fullscreen(window, cx);
            cx.stop_propagation();
            return;
        }
        let text = self.text_focused(window, cx);
        let dir = match key {
            "up" => Some(Dir::Up),
            "down" => Some(Dir::Down),
            "left" => Some(Dir::Left),
            "right" => Some(Dir::Right),
            _ => None,
        };
        if let Some(d) = dir {
            if let Some(input) = &text
                && d.is_horizontal()
            {
                // Leave a text field only when the caret is already at its edge.
                let s = input.read(cx);
                let sel = s.selected_range();
                let len = s.value().len();
                let at_start = sel.start == 0 && sel.end == 0;
                let at_end = sel.start == len;
                if (d == Dir::Left && !at_start) || (d == Dir::Right && !at_end) {
                    return;
                }
            }
            self.set_mode(InputMode::Keyboard, cx);
            self.move_focus(d, window, cx);
            cx.stop_propagation();
            return;
        }
        if key == "escape" || (key == "backspace" && text.is_none()) {
            self.set_mode(InputMode::Keyboard, cx);
            if text.is_some() {
                self.focus_handle.focus(window, cx);
            }
            self.back(window, cx);
            cx.stop_propagation();
            return;
        }
        if text.is_some() {
            return;
        }
        match key {
            "enter" | "space" => {
                self.set_mode(InputMode::Keyboard, cx);
                if !ev.is_held {
                    self.activate(window, cx);
                }
                cx.stop_propagation();
            }
            "pageup" | "q" => self.cycle_tab(-1, window, cx),
            "pagedown" | "e" => self.cycle_tab(1, window, cx),
            "f" | "/" => {
                self.secondary(window, cx);
                cx.stop_propagation();
            }
            "tab" => {
                self.set_mode(InputMode::Keyboard, cx);
                self.move_focus(if m.shift { Dir::Up } else { Dir::Down }, window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn on_pad(&mut self, cmd: gamepad::PadCommand, kind: gamepad::PadKind, window: &mut Window, cx: &mut Context<Self>) {
        // Ignore input while another window (the emulator) has the focus.
        if !window.is_window_active() {
            return;
        }
        self.set_mode(InputMode::Gamepad, cx);
        if self.pad_kind != kind {
            self.pad_kind = kind;
            cx.notify();
        }
        use gamepad::PadCommand as P;
        match cmd {
            P::Move(d) => {
                // A text field keeps the caret moves of the keyboard only.
                if self.text_focused(window, cx).is_some() {
                    self.focus_handle.focus(window, cx);
                }
                self.move_focus(d, window, cx);
            }
            P::A => self.activate(window, cx),
            P::B => {
                if self.text_focused(window, cx).is_some() {
                    self.focus_handle.focus(window, cx);
                }
                self.back(window, cx);
            }
            P::Lb => self.cycle_tab(-1, window, cx),
            P::Rb => self.cycle_tab(1, window, cx),
            P::Y => self.secondary(window, cx),
            P::X => {}
            P::Start => {
                if self.modals.is_empty() && self.state.session.is_some() {
                    self.set_tab(Tab::Settings, window, cx);
                }
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------------------ data helpers
    pub fn detail(&mut self, id: i64, cx: &mut Context<Self>) -> Option<GameDetail> {
        if let Some(d) = self.details.get(&id) {
            return d.clone();
        }
        if self.details_loading.insert(id) {
            let client = self.client.clone();
            let task = self.client.rt.spawn(async move { client.game_detail(id).await });
            cx.spawn(async move |this, cx| {
                let d = task.await.ok().flatten();
                let _ = this.update(cx, |this, cx| {
                    this.details_loading.remove(&id);
                    this.details.insert(id, d);
                    cx.notify();
                });
            })
            .detach();
        }
        None
    }

    pub fn refresh_locations(&mut self) {
        self.locations = self
            .client
            .settings()
            .locations
            .iter()
            .map(|p| {
                let ok = std::fs::create_dir_all(p).is_ok();
                LocInfo { path: p.clone(), free: if ok { fs4::available_space(p).ok() } else { None }, exists: ok }
            })
            .collect();
    }

    pub fn input_value(&self, e: &Entity<InputState>, cx: &App) -> String {
        e.read(cx).value().to_string()
    }

    pub fn save_extra_args(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.args_for else { return };
        let v = self.input_value(&self.inputs.extra_args.clone(), cx);
        let cur = self.client.game_settings(id).extra_args.unwrap_or_default();
        if v != cur {
            self.client.update_game_settings(id, |s| s.extra_args = if v.is_empty() { None } else { Some(v) });
        }
    }
}

impl Dir {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.nav.borrow_mut().begin_frame();
        let body = if !self.state.ready {
            div().size_full().bg(theme::bg()).into_any_element()
        } else if self.state.session.is_none() {
            self.render_login(window, cx).into_any_element()
        } else {
            self.render_shell(window, cx).into_any_element()
        };
        let mode = self.nav.borrow().mode;
        div()
            .id("root")
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT)
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.on_key(ev, window, cx)))
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                if mode != InputMode::Mouse {
                    this.set_mode(InputMode::Mouse, cx);
                }
            }))
            .child(body)
            .child(self.render_toasts())
            .children(self.render_modals(window, cx))
    }
}

/// Open the window and run until it is closed.
pub fn run(client: Arc<Client>, ui_rx: futures_channel::mpsc::UnboundedReceiver<UiEvent>, fullscreen: bool) {
    let app = gpui_kit::application().with_assets(assets::AppAssets);
    let client_end = client.clone();
    app.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        gpui_kit::component::theme::Theme::change(gpui_kit::component::theme::ThemeMode::Dark, None, cx);
        shell::style_components(cx);
        let bounds = Bounds::centered(None, size(px(1440.), px(880.)), cx);
        let options = WindowOptions {
            window_bounds: Some(if fullscreen { WindowBounds::Fullscreen(bounds) } else { WindowBounds::Windowed(bounds) }),
            titlebar: Some(TitlebarOptions { title: Some("Nostalgia".into()), ..Default::default() }),
            window_min_size: Some(size(px(960.), px(600.))),
            app_id: Some("nostalgia".into()),
            ..Default::default()
        };
        let client2 = client.clone();
        gpui_kit::open_window(options, cx, move |window, cx| cx.new(|cx| AppView::new(client2, ui_rx, window, cx))).expect("open window");
        let client3 = client.clone();
        cx.on_app_quit(move |_| {
            client3.flush();
            async {}
        })
        .detach();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
    client_end.flush();
}

#[cfg(test)]
mod filter_tests {
    use super::Filters;

    #[test]
    fn filters_read_old_single_values_and_toggle() {
        let f: Filters = serde_json::from_str(r#"{"platform":"psx","genre":"","theme":["Fantasy","Drama"],"sort":"year"}"#).unwrap();
        assert_eq!(f.platform, vec!["psx"]);
        assert!(f.genre.is_empty());
        assert_eq!(f.theme, vec!["Fantasy", "Drama"]);
        assert_eq!(f.sort, "year");
        let mut f = f;
        f.toggle("platform", "ps2");
        f.toggle("theme", "Fantasy");
        assert_eq!(f.platform, vec!["psx", "ps2"]);
        assert_eq!(f.theme, vec!["Drama"]);
        f.toggle("platform", "");
        assert!(f.platform.is_empty());
    }
}
