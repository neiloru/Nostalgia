//! Building blocks styled like the Electron UI, all registered with the navigation.

use super::nav::{ActionFn, NavRc};
use super::theme::surface;
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

/// An action shown in the hint bar, drawn as the button of the controller in use (Kenney Input
/// Prompts) or as the keyboard key.
#[derive(Clone, Copy, PartialEq)]
pub enum PadPrompt {
    Confirm,
    Back,
    Search,
    TabLeft,
    TabRight,
    Menu,
    Move,
}

/// PadPrompt icon for a controller family, or for the keyboard (`None`). Buttons are positional:
/// Confirm is the bottom face button, Back the right one, Search the top one.
pub fn prompt_name(pad: Option<super::gamepad::PadKind>, p: PadPrompt) -> Option<&'static str> {
    use super::gamepad::PadKind as K;
    use PadPrompt as P;
    Some(match (pad, p) {
        (None, P::Confirm) => "keyboard_enter",
        (None, P::Back) => "keyboard_escape",
        (None, P::Search) => "keyboard_f",
        (None, P::TabLeft) => "keyboard_q",
        (None, P::TabRight) => "keyboard_e",
        (None, P::Move) => "keyboard_arrows",
        (None, P::Menu) => return None,
        (Some(K::Xbox), P::Confirm) => "xbox_button_a",
        (Some(K::Xbox), P::Back) => "xbox_button_b",
        (Some(K::Xbox), P::Search) => "xbox_button_y",
        (Some(K::Xbox), P::TabLeft) => "xbox_lb",
        (Some(K::Xbox), P::TabRight) => "xbox_rb",
        (Some(K::Xbox), P::Menu) => "xbox_button_menu",
        (Some(K::Xbox), P::Move) => "xbox_dpad",
        (Some(K::PlayStation), P::Confirm) => "playstation_button_cross",
        (Some(K::PlayStation), P::Back) => "playstation_button_circle",
        (Some(K::PlayStation), P::Search) => "playstation_button_triangle",
        (Some(K::PlayStation), P::TabLeft) => "playstation_trigger_l1",
        (Some(K::PlayStation), P::TabRight) => "playstation_trigger_r1",
        (Some(K::PlayStation), P::Menu) => "playstation5_button_options",
        (Some(K::PlayStation), P::Move) => "playstation_dpad",
        // Nintendo labels: the bottom button is B, the right one A, the top one X.
        (Some(K::Switch), P::Confirm) => "switch_button_b",
        (Some(K::Switch), P::Back) => "switch_button_a",
        (Some(K::Switch), P::Search) => "switch_button_x",
        (Some(K::Switch), P::TabLeft) => "switch_button_l",
        (Some(K::Switch), P::TabRight) => "switch_button_r",
        (Some(K::Switch), P::Menu) => "switch_button_plus",
        (Some(K::Switch), P::Move) => "switch_dpad",
        (Some(K::SteamDeck), P::Confirm) => "steamdeck_button_a",
        (Some(K::SteamDeck), P::Back) => "steamdeck_button_b",
        (Some(K::SteamDeck), P::Search) => "steamdeck_button_y",
        (Some(K::SteamDeck), P::TabLeft) => "steamdeck_button_l1",
        (Some(K::SteamDeck), P::TabRight) => "steamdeck_button_r1",
        (Some(K::SteamDeck), P::Menu) => "steamdeck_button_options",
        (Some(K::SteamDeck), P::Move) => "steamdeck_dpad",
    })
}

pub fn prompt(pad: Option<super::gamepad::PadKind>, p: PadPrompt, size: f32, color: Hsla) -> Option<Svg> {
    prompt_name(pad, p).map(|n| svg().path(super::assets::prompt_path(n)).size(r(size)).flex_none().text_color(color))
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
    use gpui_kit::base::{Scrollbar, ScrollbarMode};
    nav.borrow_mut().pop_scroll();
    let area = div().id(SharedString::from(format!("scroll-{key}"))).flex_grow_1().flex_shrink(1.).min_h_0().overflow_y_scroll().track_scroll(handle).child(content);
    // Shown only when the content is taller than the area; the thumb can be dragged.
    let bar = Scrollbar::vertical(handle)
        .id(SharedString::from(format!("scrollbar-{key}")))
        .mode(ScrollbarMode::Always)
        .viewport_from_layout()
        .styles(|s| {
            s.track(|t| t.bg(transparent_black()))
                .thumb(|t| t.bg(ca(0xffffff2e)).width(px(6.)).inset(px(3.)).radius(px(3.)))
                .thumb_hover(|t| t.bg(ca(0xffffff55)).width(px(8.)).inset(px(3.)).radius(px(4.)))
                .thumb_active(|t| t.bg(ca(0xffffff70)).width(px(8.)).inset(px(3.)).radius(px(4.)))
        });
    // The callers size this wrapper; the area fills it (or shrinks to a maximum height).
    div().id(SharedString::from(format!("scrollwrap-{key}"))).relative().flex().flex_col().min_h_0().child(area).child(div().absolute().inset_0().child(bar))
}

/// The rows of a long list that are on screen, plus a few around them: only those are built, the
/// rest is replaced by empty space of the same height. `top` is where the list starts in the
/// scroll area's content and `row_h` the height of a row (including the gap), both in layout
/// units (`r`). The rows around the screen are there so the navigation can move to them.
pub fn visible_rows(handle: &ScrollHandle, window: &Window, top: f32, row_h: f32, rows: usize) -> std::ops::Range<usize> {
    rows_on_screen(handle, window, top, row_h, rows, AROUND)
}

/// Rows built around the ones on screen (see `visible_rows`).
pub const AROUND: usize = 2;

/// The rows on screen, plus `around` rows before and after them.
pub fn rows_on_screen(handle: &ScrollHandle, window: &Window, top: f32, row_h: f32, rows: usize, around: usize) -> std::ops::Range<usize> {
    let zoom = f32::from(window.rem_size()) / 16.0;
    let area = handle.bounds().size.height;
    // Not laid out yet: assume the area fills the window.
    let viewport = f32::from(if area > px(0.) { area } else { window.viewport_size().height }) / zoom;
    rows_in_view(viewport, -f32::from(handle.offset().y) / zoom, top, row_h, rows, around)
}

/// `visible_rows` for a viewport height and scroll position (layout units).
pub fn rows_in_view(viewport: f32, scroll_top: f32, top: f32, row_h: f32, rows: usize, around: usize) -> std::ops::Range<usize> {
    // The offset is corrected only while painting, e.g. after the list got shorter.
    let max_top = (top + rows as f32 * row_h - viewport).max(0.0);
    let scroll_top = scroll_top.clamp(0.0, max_top);
    let first = ((scroll_top - top) / row_h).floor().max(0.0) as usize;
    let end = ((scroll_top + viewport - top) / row_h).ceil().max(0.0) as usize;
    first.saturating_sub(around).min(rows)..(end + around).min(rows)
}

#[cfg(test)]
mod tests {
    use super::{AROUND, rows_in_view};

    #[test]
    fn only_rows_on_screen_and_two_around_them_are_built() {
        // 100 rows of 50 below a 100 high heading, 400 visible.
        assert_eq!(rows_in_view(400., 0., 100., 50., 100, AROUND), 0..8);
        // Scrolled to row 20 (y = 100 + 20 * 50).
        assert_eq!(rows_in_view(400., 1100., 100., 50., 100, AROUND), 18..30);
        // Beyond the end (the list got shorter): the last rows.
        assert_eq!(rows_in_view(400., 99_999., 100., 50., 100, AROUND), 90..100);
        assert_eq!(rows_in_view(400., 500., 100., 50., 0, AROUND), 0..0);
        // A short list.
        assert_eq!(rows_in_view(400., 0., 0., 50., 3, AROUND), 0..3);
        // Without the rows around: only those on screen.
        assert_eq!(rows_in_view(400., 1100., 100., 50., 100, 0), 20..28);
    }
}

/// Records the height of its parent (in layout units) for layouts that depend on it; draws the
/// next frame again when it changed.
pub fn measure_height(cell: Rc<std::cell::Cell<f32>>) -> impl IntoElement {
    canvas(
        move |b, window, _| {
            let h = f32::from(b.size.height) / (f32::from(window.rem_size()) / 16.0);
            if (h - cell.get()).abs() > 0.5 {
                cell.set(h);
                window.on_next_frame(|window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
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
