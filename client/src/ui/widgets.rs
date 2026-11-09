//! Building blocks styled like the Electron UI, all registered with the navigation.

use super::nav::{ActionFn, NavRc};
use super::theme::*;
use super::AppView;
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::rc::Rc;

pub fn action(f: impl Fn(&mut AppView, &mut Window, &mut Context<AppView>) + 'static) -> ActionFn {
    Rc::new(f)
}

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(super::assets::icon_path(name)).size(r(size)).flex_none().text_color(color)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Ring {
    /// Ring around the element (buttons, inputs), like `outline-offset: 3px`.
    Outer(f32),
    /// Ring inside the element (rows in lists, so it is never clipped).
    Inset(f32),
    /// The element draws its own focus state.
    None,
}

/// Register `el` as focusable: records its bounds, draws the focus ring, runs `action` on click / A.
pub fn focusable(el: Stateful<Div>, nav: &NavRc, id: impl Into<SharedString>, ring: Ring, act: Option<ActionFn>, cx: &mut Context<AppView>) -> Stateful<Div> {
    focusable_ext(el, nav, id.into(), ring, act, false, false, cx)
}

#[allow(clippy::too_many_arguments)]
pub fn focusable_ext(
    el: Stateful<Div>,
    nav: &NavRc,
    id: SharedString,
    ring: Ring,
    act: Option<ActionFn>,
    default: bool,
    autofocus: bool,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let (show_ring, bounds) = {
        let mut n = nav.borrow_mut();
        n.add(id.clone(), act.clone(), None, default, autofocus);
        (n.ring(&id), n.bounds.clone())
    };
    let key: SharedString = format!("i:{id}").into();
    let mut el = el.relative().child(
        canvas(
            move |b, _, _| {
                bounds.borrow_mut().insert(key, b);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full(),
    );
    if show_ring {
        el = el.child(ring_overlay(ring));
    }
    let click_id = id.clone();
    el.cursor_pointer().on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
        this.nav.borrow_mut().set_focus_id(&click_id);
        if let Some(a) = &act {
            a(this, window, cx);
        }
        cx.notify();
    }))
}

pub fn ring_overlay(ring: Ring) -> Div {
    match ring {
        Ring::Outer(radius) => div()
            .absolute()
            .top(r(-6.))
            .left(r(-6.))
            .right(r(-6.))
            .bottom(r(-6.))
            .border(r(RING))
            .border_color(focus())
            .rounded(r(radius + 6.)),
        Ring::Inset(radius) => div().absolute().inset_0().border(r(RING)).border_color(focus()).rounded(r(radius)),
        Ring::None => div(),
    }
}

/// A region for spatial navigation (records its bounds).
pub fn group(nav: &NavRc, key: &str, remember: bool, build: impl FnOnce() -> Div) -> Div {
    nav.borrow_mut().push_group(key.to_string().into(), remember);
    let el = build();
    nav.borrow_mut().pop_group();
    with_bounds(el, nav, format!("g:{key}"))
}

pub fn with_bounds<E: ParentElement + Styled>(el: E, nav: &NavRc, key: String) -> E {
    let bounds = nav.borrow().bounds.clone();
    let key: SharedString = key.into();
    el.relative().child(
        canvas(
            move |b, _, _| {
                bounds.borrow_mut().insert(key, b);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full(),
    )
}

/// A vertically scrolling area whose focused children are scrolled into view: `scroll_begin`,
/// build the children, then `scroll_end` with them. Items register with the scroll area (and with
/// groups) while they are created, so children built before `scroll_begin` never scroll into view.
pub fn scroll_begin(nav: &NavRc, key: &str, handle: &ScrollHandle) {
    nav.borrow_mut().push_scroll(key.to_string().into(), handle.clone());
}

pub fn scroll_end(nav: &NavRc, key: &str, handle: &ScrollHandle, content: impl IntoElement) -> Stateful<Div> {
    nav.borrow_mut().pop_scroll();
    div().id(SharedString::from(format!("scroll-{key}"))).overflow_y_scroll().track_scroll(handle).child(content)
}

/// `group` in two steps (see `scroll_begin`).
pub fn group_begin(nav: &NavRc, key: &str, remember: bool) {
    nav.borrow_mut().push_group(key.to_string().into(), remember);
}

pub fn group_end(nav: &NavRc, key: &str, el: Div) -> Div {
    nav.borrow_mut().pop_group();
    with_bounds(el, nav, format!("g:{key}"))
}

#[derive(Clone, Copy, PartialEq)]
pub enum BtnKind {
    Normal,
    Primary,
    Play,
    Danger,
    DangerPrimary,
    Ghost,
    Playing,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BtnSize {
    Small,
    Normal,
    Big,
}

pub struct Btn {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<&'static str>,
    pub hint: Option<SharedString>,
    pub kind: BtnKind,
    pub size: BtnSize,
    pub disabled: bool,
    pub default: bool,
    pub autofocus: bool,
    pub progress: Option<f32>,
    pub full: bool,
}

pub fn btn(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Btn {
    Btn {
        id: id.into(),
        label: label.into(),
        icon: None,
        hint: None,
        kind: BtnKind::Normal,
        size: BtnSize::Normal,
        disabled: false,
        default: false,
        autofocus: false,
        progress: None,
        full: false,
    }
}

impl Btn {
    pub fn icon(mut self, i: &'static str) -> Self {
        self.icon = Some(i);
        self
    }
    pub fn hint(mut self, h: impl Into<SharedString>) -> Self {
        self.hint = Some(h.into());
        self
    }
    pub fn kind(mut self, k: BtnKind) -> Self {
        self.kind = k;
        self
    }
    pub fn primary(self) -> Self {
        self.kind(BtnKind::Primary)
    }
    pub fn ghost(self) -> Self {
        self.kind(BtnKind::Ghost)
    }
    pub fn danger(self) -> Self {
        self.kind(BtnKind::Danger)
    }
    pub fn big(mut self) -> Self {
        self.size = BtnSize::Big;
        self
    }
    pub fn small(mut self) -> Self {
        self.size = BtnSize::Small;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }
    pub fn progress(mut self, p: f32) -> Self {
        self.progress = Some(p);
        self
    }
    pub fn full(mut self) -> Self {
        self.full = true;
        self
    }

    pub fn render(self, nav: &NavRc, act: ActionFn, cx: &mut Context<AppView>) -> Stateful<Div> {
        let (h, padx, fs, weight, icon_size) = match self.size {
            BtnSize::Small => (32., 12., 13., FontWeight::MEDIUM, 14.),
            BtnSize::Normal => (40., 18., 14.5, FontWeight::MEDIUM, 16.),
            BtnSize::Big => (52., 26., 16., FontWeight::SEMIBOLD, 18.),
        };
        let (mut bgc, mut fg, hover) = match self.kind {
            BtnKind::Normal => (surface3(), text(), surface4()),
            BtnKind::Primary => (text(), bg(), c(0xffffff)),
            BtnKind::Play => (play(), play_ink(), c(0x45d08a)),
            BtnKind::Danger => (surface3(), err_soft(), surface4()),
            BtnKind::DangerPrimary => (c(0xb33a3a), c(0xffffff), c(0xc44545)),
            BtnKind::Ghost => (ca(0x00000000), muted(), surface2()),
            BtnKind::Playing => (surface3(), text(), surface4()),
        };
        if self.disabled && matches!(self.kind, BtnKind::Primary | BtnKind::Play) {
            bgc = surface3();
            fg = muted();
        }
        let ghost = self.kind == BtnKind::Ghost;
        let mut el = div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(r(8.))
            .h(r(h))
            .px(r(padx))
            .rounded(r(RADIUS))
            .bg(bgc)
            .text_color(fg)
            .text_size(r(fs))
            .font_weight(weight)
            .whitespace_nowrap();
        if self.full {
            el = el.w_full();
        }
        if let Some(p) = self.progress {
            el = el.child(div().absolute().left_0().top_0().bottom_0().w(relative(p.clamp(0.0, 1.0))).rounded(r(RADIUS)).bg(ca(0x4c9df04d)));
        }
        if let Some(i) = self.icon {
            el = el.child(icon(i, icon_size, fg));
        }
        el = el.child(div().child(self.label));
        if let Some(h) = self.hint {
            el = el.child(div().font_weight(FontWeight::NORMAL).opacity(0.7).child(h));
        }
        if self.disabled {
            let o = if matches!(self.kind, BtnKind::Primary | BtnKind::Play) { 1.0 } else { 0.4 };
            // Disabled buttons stay focusable for orientation but do nothing.
            let id = self.id.clone();
            let el = el.opacity(o);
            return focusable_ext(el, nav, id, Ring::Outer(RADIUS), None, self.default, self.autofocus, cx).cursor_default();
        }
        let el = if ghost { el.hover(move |s| s.bg(hover).text_color(text())) } else { el.hover(move |s| s.bg(hover)) };
        focusable_ext(el, nav, self.id, Ring::Outer(RADIUS), Some(act), self.default, self.autofocus, cx)
    }
}

/// An icon-only button (close, previous / next).
pub fn icon_btn(id: impl Into<SharedString>, name: &'static str, nav: &NavRc, act: ActionFn, cx: &mut Context<AppView>) -> Stateful<Div> {
    let id = id.into();
    let el = div()
        .id(id.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(r(40.))
        .rounded(r(RADIUS))
        .text_color(muted())
        .hover(|s| s.bg(surface2()).text_color(text()))
        .child(icon(name, 18., muted()));
    focusable(el, nav, id, Ring::Outer(RADIUS), Some(act), cx)
}

/// Switch with a label and an optional hint line.
pub fn toggle(id: impl Into<SharedString>, label: impl Into<SharedString>, hint: Option<SharedString>, on: bool, nav: &NavRc, act: ActionFn, cx: &mut Context<AppView>) -> Stateful<Div> {
    let id: SharedString = id.into();
    let focused = nav.borrow().ring(&id);
    let track = div()
        .relative()
        .flex_none()
        .w(r(38.))
        .h(r(22.))
        .rounded_full()
        .bg(if on { play() } else { surface3() })
        .child(div().absolute().top(r(3.)).left(r(if on { 19. } else { 3. })).size(r(16.)).rounded_full().bg(if on { c(0xffffff) } else { muted() }));
    let mut label_col = div().flex().flex_col().gap(r(2.)).text_size(r(14.5)).text_color(text()).child(label.into());
    if let Some(h) = hint {
        label_col = label_col.child(div().text_size(r(12.5)).text_color(muted()).child(h));
    }
    let el = div()
        .id(id.clone())
        .flex()
        .items_center()
        .gap(r(14.))
        .w_full()
        .px(r(8.))
        .py(r(10.))
        .rounded(r(RADIUS))
        .when(focused, |d| d.bg(surface3()))
        .hover(|s| s.bg(surface2()))
        .child(track)
        .child(label_col);
    focusable(el, nav, id, Ring::Inset(RADIUS), Some(act), cx)
}

/// Controller friendly select: a button showing label and value; `act` opens the option list.
pub fn select(id: impl Into<SharedString>, label: &str, value: &str, nav: &NavRc, act: ActionFn, cx: &mut Context<AppView>) -> Stateful<Div> {
    let id: SharedString = id.into();
    let focused = nav.borrow().ring(&id);
    let el = div()
        .id(id.clone())
        .flex()
        .items_center()
        .gap(r(8.))
        .w_full()
        .px(r(14.))
        .py(r(9.))
        .rounded(r(RADIUS))
        .bg(if focused { surface3() } else { surface2() })
        .hover(|s| s.bg(surface3()))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().text_size(r(12.)).text_color(muted()).child(label.to_string()))
                .child(div().text_size(r(14.5)).font_weight(FontWeight::MEDIUM).text_color(text()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(value.to_string())),
        )
        .child(div().text_size(r(12.)).text_color(muted()).child("▾"));
    focusable(el, nav, id, Ring::Inset(RADIUS), Some(act), cx)
}

pub fn badge(label: impl Into<SharedString>, bgc: Hsla, fg: Hsla) -> Div {
    div().flex().items_center().gap(r(4.)).px(r(7.)).py(r(3.)).rounded(r(5.)).bg(bgc).text_color(fg).text_size(r(11.5)).font_weight(FontWeight::SEMIBOLD).child(label.into())
}

pub fn chip(label: impl Into<SharedString>) -> Div {
    div().px(r(10.)).py(r(4.)).rounded(r(5.)).bg(ca(0xffffff1a)).text_size(r(13.)).text_color(text()).child(label.into())
}

pub fn progress(value: u64, total: u64, paused: bool) -> Div {
    let pct = if total > 0 { (value as f32 / total as f32).min(1.0) } else { 0.0 };
    div()
        .w_full()
        .h(r(6.))
        .rounded_full()
        .bg(surface3())
        .overflow_hidden()
        .child(div().h_full().w(relative(pct)).rounded_full().bg(if paused { faint() } else { info() }))
}

pub fn kbd(label: &str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(r(24.))
        .h(r(24.))
        .px(r(6.))
        .rounded_full()
        .border(r(1.5))
        .border_color(faint())
        .text_color(text())
        .text_size(r(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.to_string())
}

pub fn h1(t: impl Into<SharedString>) -> Div {
    div().text_size(r(28.)).font_weight(FontWeight::SEMIBOLD).text_color(text()).child(t.into())
}

pub fn h2(t: impl Into<SharedString>) -> Div {
    div().flex().items_center().gap(r(10.)).mb(r(14.)).text_size(r(17.)).font_weight(FontWeight::SEMIBOLD).text_color(text()).child(t.into())
}

pub fn h3(t: impl Into<SharedString>) -> Div {
    div().mt(r(22.)).mb(r(10.)).text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(muted()).child(t.into())
}

pub fn panel() -> Div {
    div().flex().flex_col().p(r(22.)).rounded(r(10.)).bg(surface()).min_w_0()
}

/// Two-column key / value list (`.kv`).
pub fn kv(rows: Vec<(String, String)>) -> Div {
    let mut d = div().flex().flex_col().gap(r(8.)).mb(r(14.)).text_size(r(14.));
    for (k, v) in rows {
        d = d.child(div().flex().gap(r(14.)).child(div().w(r(140.)).flex_none().text_color(muted()).child(k)).child(div().flex_1().min_w_0().text_color(text()).child(v)));
    }
    d
}
