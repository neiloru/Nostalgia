//! Pages and the pieces they share (port of common.tsx).

mod downloads;
mod library;
mod login;
mod settings;
mod store;
mod vault;

use super::nav::NavRc;
use super::theme::*;
use super::widgets::*;
use super::{AppView, Modal};
use crate::model::{ClientState, DownloadJob, DownloadStatus};
use crate::util::fmt_bytes;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::{EmulatorSummary, GameSummary, ItemKind};
use std::sync::Arc;

#[derive(Clone)]
pub enum AnyItem {
    Game(GameSummary),
    Emu(EmulatorSummary),
}

impl AnyItem {
    pub fn kind(&self) -> ItemKind {
        match self {
            AnyItem::Game(_) => ItemKind::Game,
            AnyItem::Emu(_) => ItemKind::Emulator,
        }
    }
    pub fn id(&self) -> i64 {
        match self {
            AnyItem::Game(g) => g.id,
            AnyItem::Emu(e) => e.id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            AnyItem::Game(g) => &g.name,
            AnyItem::Emu(e) => &e.name,
        }
    }
    pub fn platform(&self) -> &str {
        match self {
            AnyItem::Game(g) => &g.platform,
            AnyItem::Emu(e) => &e.platform,
        }
    }
    pub fn platform_name(&self) -> &str {
        match self {
            AnyItem::Game(g) => &g.platform_name,
            AnyItem::Emu(e) => &e.platform_name,
        }
    }
    pub fn added_at(&self) -> &str {
        match self {
            AnyItem::Game(g) => &g.added_at,
            AnyItem::Emu(e) => &e.added_at,
        }
    }
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind().as_str(), self.id())
    }
}

pub struct Status<'a> {
    pub installed: bool,
    pub job: Option<&'a DownloadJob>,
    pub in_library: bool,
}

pub fn item_status<'a>(state: &ClientState, jobs: &'a [DownloadJob], kind: ItemKind, id: i64) -> Status<'a> {
    Status {
        installed: state.installed(kind, id).is_some(),
        job: jobs.iter().find(|j| j.kind == kind && j.id == id && j.status != DownloadStatus::Done),
        in_library: state.in_library(kind, id),
    }
}

pub fn pct(j: &DownloadJob) -> u32 {
    ((j.received_bytes as f64 / j.total_bytes.max(1) as f64) * 100.0).round() as u32
}

impl AppView {
    /// Cover image (or a placeholder with the name) of a game / emulator, filling its parent.
    /// `radius` rounds the image itself: GPUI clips children of a rounded parent to a rectangle.
    pub fn cover(&mut self, item: &AnyItem, small: bool, radius: f32) -> AnyElement {
        match item {
            AnyItem::Emu(e) => {
                let mut d = div().size_full().rounded(r(radius)).flex().flex_col().items_center().justify_center().gap(r(10.)).p(r(14.)).bg(surface2()).child(icon("chip", 42., muted()));
                if !small {
                    d = d
                        .child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).text_color(text()).text_center().child(e.name.clone()))
                        .child(div().text_size(r(12.)).text_color(muted()).child(e.platform_name.clone()));
                }
                d.into_any_element()
            }
            AnyItem::Game(g) => {
                if g.has_cover
                    && let Some(p) = self.client.media(g.id, "cover.jpg")
                {
                    return img(p).size_full().rounded(r(radius)).object_fit(ObjectFit::Cover).into_any_element();
                }
                let mut d = div().size_full().rounded(r(radius)).flex().flex_col().justify_end().gap(r(4.)).p(r(14.)).bg(surface3()).text_size(r(15.));
                if !small {
                    d = d.child(div().font_weight(FontWeight::BOLD).child(g.name.clone())).child(div().text_size(r(12.)).text_color(muted()).child(g.platform_name.clone()));
                }
                d.into_any_element()
            }
        }
    }

    /// Card in the vault grid.
    pub fn game_card(&mut self, item: &AnyItem, width: f32, autofocus: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let st = item_status(&state, &jobs, item.kind(), item.id());
        let disabled = !state.online && !st.installed;
        let id: SharedString = format!("card:{}", item.key()).into();
        let focused = nav.borrow().ring(&id);
        let cover_h = width * 4.0 / 3.0;
        let mut badges = div().absolute().top(r(8.)).left(r(8.)).right(r(8.)).flex().justify_end().gap(r(6.));
        if st.installed {
            badges = badges.child(div().flex().items_center().px(r(7.)).py(r(3.)).rounded(r(5.)).bg(play()).child(icon("check", 12., play_ink())));
        }
        if let Some(j) = st.job {
            badges = badges.child(badge(if j.status == DownloadStatus::Paused { "Paused".to_string() } else { format!("{}%", pct(j)) }, info(), c(0x06121e)));
        }
        if st.in_library && !st.installed {
            badges = badges.child(badge("In library", ca(0x0f0f11d9), text()));
        }
        let mut cover = div()
            .relative()
            .w(r(width))
            .h(r(cover_h))
            .rounded(r(RADIUS))
            .overflow_hidden()
            .bg(surface2())
            .child(self.cover(item, false, RADIUS))
            .child(badges);
        if let Some(j) = st.job {
            cover = cover.child(
                div().absolute().left_0().right_0().bottom_0().h(r(4.)).bg(ca(0x00000099)).child(div().h_full().w(relative(pct(j) as f32 / 100.0)).bg(info())),
            );
        }
        let mut cover_wrap = div().relative().child(cover);
        if focused {
            cover_wrap = cover_wrap.child(ring_overlay(Ring::Outer(RADIUS)).top(r(-7.)).left(r(-7.)).right(r(-7.)).bottom(r(-7.)));
        }
        let mut sub = div().flex().gap(r(8.)).text_size(r(12.5)).text_color(muted()).mt(r(-4.)).child(item.platform().to_uppercase());
        match item {
            AnyItem::Game(g) => {
                if let Some(y) = g.year {
                    sub = sub.child(y.to_string());
                }
            }
            AnyItem::Emu(_) => sub = sub.child("Emulator"),
        }
        let el = div()
            .id(id.clone())
            .flex()
            .flex_col()
            .gap(r(10.))
            .w(r(width))
            .when(disabled, |d| d.opacity(0.3))
            .child(cover_wrap)
            .child(
                div()
                    .text_size(r(14.5))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(r(19.))
                    .text_color(if focused { c(0xffffff) } else { text2() })
                    .line_clamp(2)
                    .child(item.name().to_string()),
            )
            .child(sub);
        let (kind, iid, key) = (item.kind(), item.id(), item.key());
        let act = super::widgets::action(move |this, window, cx| {
            this.last_card = Some(key.clone());
            this.open_store(kind, iid, window, cx);
        });
        focusable_ext(el, &nav, id, Ring::None, Some(act), false, autofocus, cx)
    }

    /// Install / Play / progress button + library buttons (store page and library).
    pub fn action_buttons(&mut self, kind: ItemKind, id: i64, size: u64, show_library: bool, open_library: bool, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let st = item_status(&state, &jobs, kind, id);
        let running = kind == ItemKind::Game && state.running.as_ref().is_some_and(|r| r.game_id == id);
        let client = self.client.clone();
        let main = if running {
            let c = client.clone();
            btn("act:main", "Playing – Quit").big().kind(BtnKind::Playing).render(&nav, action(move |_, _, _| c.stop_game()), cx)
        } else if st.installed && kind == ItemKind::Game {
            let c = client.clone();
            btn("act:main", "Play")
                .big()
                .kind(BtnKind::Play)
                .icon("play")
                .autofocus()
                .disabled(state.running.is_some())
                .render(&nav, action(move |this, _, cx| {
                    let c = c.clone();
                    this.act(async move { c.launch(id).await }, None, cx);
                }), cx)
        } else if st.installed {
            btn("act:main", "Installed").big().icon("check").disabled(true).render(&nav, action(|_, _, _| {}), cx)
        } else if let Some(j) = st.job {
            let p = pct(j);
            let key = j.key.clone();
            let c = client.clone();
            if matches!(j.status, DownloadStatus::Paused | DownloadStatus::Error) {
                btn("act:main", format!("Resume ({p}%)")).big().primary().icon("download").autofocus().render(&nav, action(move |_, _, _| c.resume_download(&key)), cx)
            } else {
                let label = if j.status == DownloadStatus::Queued { "Queued".to_string() } else { format!("Downloading {p}%") };
                btn("act:main", label).big().icon("pause").progress(p as f32 / 100.0).autofocus().render(&nav, action(move |_, _, _| c.pause_download(&key)), cx)
            }
        } else {
            let c = client.clone();
            btn("act:main", "Install")
                .big()
                .primary()
                .icon("download")
                .hint(fmt_bytes(size))
                .autofocus()
                .disabled(!state.online)
                .render(&nav, action(move |this, window, cx| {
                    if c.settings().locations.len() > 1 {
                        this.open_modal(Modal::InstallTo { kind, id }, window, cx);
                    } else {
                        let c = c.clone();
                        this.act(async move { c.install(kind, id, None).await }, None, cx);
                    }
                }), cx)
        };
        let mut row = div().flex().flex_wrap().items_center().gap(r(10.)).child(main);
        if show_library {
            if st.in_library {
                if open_library {
                    row = row.child(btn("act:open-lib", "Open in library").big().render(&nav, action(move |this, window, cx| this.open_in_library(kind, id, window, cx)), cx));
                }
                let installed = st.installed;
                row = row.child(btn("act:in-lib", "In library").big().icon("check").disabled(!state.online).render(
                    &nav,
                    action(move |this, window, cx| this.confirm_remove_from_library(kind, id, installed, window, cx)),
                    cx,
                ));
            } else {
                let c = client.clone();
                row = row.child(btn("act:add-lib", "Add to library").big().icon("plus").disabled(!state.online).render(
                    &nav,
                    action(move |this, _, cx| {
                        let c = c.clone();
                        this.act(async move { c.add_to_library(kind, id).await }, Some("Added to your library"), cx);
                    }),
                    cx,
                ));
            }
        }
        row
    }

    pub fn confirm_remove_from_library(&mut self, kind: ItemKind, id: i64, installed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let msg = if installed {
            "The game stays installed on this device. Cloud saves are kept on the server."
        } else {
            "You can add it again at any time from the vault."
        };
        self.open_modal(
            Modal::Confirm {
                title: "Remove from library?".into(),
                message: msg.into(),
                confirm: "Remove".into(),
                danger: true,
                on_confirm: action(move |this, _, cx| {
                    let c = client.clone();
                    this.act(async move { c.remove_from_library(kind, id).await }, None, cx);
                }),
            },
            window,
            cx,
        );
    }

    pub fn offline_banner(&mut self, cx: &mut Context<Self>) -> Div {
        let client = self.client.clone();
        div()
            .flex()
            .items_center()
            .gap(r(14.))
            .px(r(16.))
            .py(r(12.))
            .mb(r(22.))
            .rounded(r(RADIUS))
            .bg(surface())
            .border_1()
            .border_l(r(3.))
            .border_color(line())
            .text_size(r(14.))
            .text_color(text2())
            .child(icon("wifi-off", 18., err()))
            .child(
                div()
                    .flex_1()
                    .child(div().flex().flex_wrap().gap(r(4.)).child(div().font_weight(FontWeight::BOLD).child("You are offline.")).child(
                        "Only installed games are available. Play time and cloud saves are uploaded automatically when the server is reachable again.",
                    )),
            )
            .child(btn("offline:retry", "Retry").small().icon("refresh").render(&self.nav.clone(), action(move |this, _, cx| {
                let c = client.clone();
                this.act(async move { Ok::<_, crate::client::ApiError>(c.refresh().await) }, None, cx);
            }), cx))
    }

    pub fn open_store(&mut self, kind: ItemKind, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = super::Tab::Vault;
        self.store_sel = Some((kind, id));
        if kind == ItemKind::Game {
            self.details.remove(&id);
        }
        self.scroll("store").set_offset(point(px(0.), px(0.)));
        self.request_focus(super::FocusTarget::FirstIn("store"), window, cx);
    }

    pub fn close_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store_sel = None;
        match self.last_card.clone() {
            Some(k) => self.request_focus(super::FocusTarget::Id(format!("card:{k}")), window, cx),
            None => self.request_focus(super::FocusTarget::FirstIn("vault-main"), window, cx),
        }
    }

    pub fn open_in_library(&mut self, kind: ItemKind, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.lib_sel = Some((kind, id));
        self.tab = super::Tab::Library;
        self.request_focus(super::FocusTarget::FirstIn("lib-detail"), window, cx);
    }
}

/// Text field registered with the navigation (A / Enter focuses it, typing works as usual).
pub fn text_input(nav: &NavRc, id: &str, state: &Entity<InputState>, search: bool, cx: &mut Context<AppView>) -> Stateful<Div> {
    let ring = nav.borrow().ring(id);
    nav.borrow_mut().add(id.to_string().into(), None, Some(state.clone()), false, false);
    let bounds = nav.borrow().bounds.clone();
    let key: SharedString = format!("i:{id}").into();
    let _ = cx;
    let mut row = div()
        .id(SharedString::from(format!("input-{id}")))
        .relative()
        .flex()
        .items_center()
        .gap(r(10.))
        .h(r(44.))
        .px(r(if search { 12. } else { 14. }))
        .rounded(r(RADIUS))
        .bg(bg())
        .border_1()
        .border_color(line())
        .text_size(r(15.));
    if search {
        row = row.child(icon("search", 16., muted()));
    }
    row = row.child(div().flex_1().child(Input::new(state).appearance(false)));
    row = row.child(
        canvas(
            move |b, _, _| {
                bounds.borrow_mut().insert(key, b);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full(),
    );
    if ring {
        row = row.child(ring_overlay(Ring::Outer(RADIUS)));
    }
    row
}

/// Label above a field (`.field`).
pub fn field(label: &str, input: impl IntoElement) -> Div {
    div().flex().flex_col().gap(r(6.)).text_size(r(13.)).text_color(muted()).child(label.to_string()).child(input)
}

pub fn view_width(window: &Window, state: &Arc<ClientState>) -> f32 {
    f32::from(window.viewport_size().width) / state.ui_zoom.max(0.5) as f32
}
