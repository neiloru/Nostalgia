//! Top bar, running game banner, hint bar, toasts and dialogs (port of App.tsx and ui.tsx).

use super::nav::InputMode;
use super::theme::surface;
use super::theme::*;
use super::widgets::*;
use super::{AppView, FocusTarget, Modal, TABS, Tab};
use crate::model::DownloadStatus;
use crate::util::{fmt_bytes, fmt_clock, fmt_date};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::ItemKind;

/// Colours of the gpui-kit components (text inputs) matching the rest of the UI.
pub fn style_components(cx: &mut App) {
    let t = gpui_kit::component::theme::Theme::global_mut(cx);
    t.font_family = FONT.into();
    t.font_size = px(15.);
    t.radius = px(RADIUS);
    let colors = &mut **t;
    colors.background = bg();
    colors.foreground = text();
    colors.input = line();
    colors.border = line();
    colors.ring = muted();
    colors.muted_foreground = muted();
    colors.caret = text();
    colors.selection = ca(0x4c9df066);
    colors.primary = text();
}

impl AppView {
    pub fn render_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let content = group(&self.nav.clone(), "content", true, || {
            div().flex_1().min_h_0().overflow_hidden().child(match self.tab {
                Tab::Vault => self.render_vault(window, cx).into_any_element(),
                Tab::Library => self.render_library(window, cx).into_any_element(),
                Tab::Downloads => self.render_downloads(window, cx).into_any_element(),
                Tab::Settings => self.render_settings(window, cx).into_any_element(),
            })
        })
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_topbar(cx))
            .children(self.render_running(cx))
            .child(content)
            .child(self.render_hintbar())
    }

    fn render_topbar(&mut self, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let gamepad = nav.borrow().mode == InputMode::Gamepad;
        let active: Vec<&DownloadStatus> = self.jobs.iter().map(|j| &j.status).filter(|s| matches!(s, DownloadStatus::Downloading | DownloadStatus::Queued)).collect();
        let dl_pct = {
            let act: Vec<_> = self.jobs.iter().filter(|j| matches!(j.status, DownloadStatus::Downloading | DownloadStatus::Queued)).collect();
            let total: u64 = act.iter().map(|j| j.total_bytes).sum();
            let got: u64 = act.iter().map(|j| j.received_bytes).sum();
            if total > 0 { (got as f64 / total as f64 * 100.0).round() as u32 } else { 0 }
        };
        let tab = self.tab;
        let state = self.state.clone();
        let user = state.session.as_ref().map(|(_, u)| u.username.clone()).unwrap_or_default();
        group(&nav, "bar", false, || {
            let pad_kind = self.pad_kind;
            let pad_hint = |p: PadPrompt| div().mx(r(4.)).flex().items_center().children(prompt(Some(pad_kind), p, 34., muted()));
            let mut tabs = div().flex().items_center().gap(r(4.)).flex_1();
            if gamepad {
                tabs = tabs.child(pad_hint(PadPrompt::TabLeft));
            }
            for (t, label) in TABS {
                let id: SharedString = format!("tab:{label}").into();
                let is_active = t == tab;
                let mut el = div()
                    .id(id.clone())
                    .flex()
                    .items_center()
                    .gap(r(8.))
                    .h(r(42.))
                    .px(r(16.))
                    .rounded(r(RADIUS))
                    .text_size(r(16.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if is_active { text() } else { muted() })
                    .hover(|s| s.text_color(text()).bg(surface2()))
                    .child(label);
                if t == Tab::Downloads && !active.is_empty() {
                    el = el.child(div().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).text_color(info()).child(format!("{dl_pct}%")));
                }
                if is_active {
                    el = el.child(div().absolute().left(r(16.)).right(r(16.)).bottom(r(-11.)).h(r(2.)).bg(text()));
                }
                let act = action(move |this, window, cx| {
                    if this.tab == t {
                        this.request_focus(FocusTarget::FirstIn("content"), window, cx);
                    } else {
                        this.set_tab(t, window, cx);
                    }
                });
                tabs = tabs.child(focusable_ext(el, &nav, id, Ring::Outer(RADIUS), Some(act), is_active, false, cx));
            }
            if gamepad {
                tabs = tabs.child(pad_hint(PadPrompt::TabRight));
            }
            let mut right = div().flex().items_center().gap(r(12.));
            if !state.online {
                let c2 = self.client.clone();
                right = right.child(
                    btn("bar:offline", "Offline")
                        .small()
                        .danger()
                        .icon("wifi-off")
                        .render(&nav, action(move |this, _, cx| {
                            let c = c2.clone();
                            this.act(async move { Ok::<_, crate::client::ApiError>(c.refresh().await) }, None, cx);
                        }), cx),
                );
            }
            right = right.child(div().text_size(r(14.)).text_color(muted()).child(user.clone()));
            right = right.child(icon_btn("bar:fullscreen", "expand", &nav, action(|this, window, cx| this.toggle_fullscreen(window, cx)), cx));
            div().flex().items_center().gap(r(24.)).h(r(64.)).px(r(24.)).bg(bg()).border_b_1().border_color(line()).flex_none().child(tabs).child(right)
        })
    }

    fn render_running(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let r_ = self.state.running.clone()?;
        let started = nostalgia_shared::parse_iso_millis(&r_.started_at).unwrap_or(0);
        let secs = (crate::util::now_ms() - started) / 1000;
        // Tick once a second while a game runs.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        let client = self.client.clone();
        Some(
            div()
                .flex()
                .items_center()
                .gap(r(14.))
                .px(r(24.))
                .py(r(10.))
                .bg(surface())
                .border_b_1()
                .border_color(line())
                .text_size(r(14.5))
                .flex_none()
                .child(div().size(r(8.)).rounded_full().bg(play()))
                .child(div().flex().gap(r(5.)).child("Playing").child(div().font_weight(FontWeight::BOLD).child(r_.name.clone())))
                .child(div().font_family(MONO).text_size(r(12.5)).child(fmt_clock(secs)))
                .child(div().flex_1())
                .child(btn("banner:quit", "Force quit").small().danger().icon("x").render(&self.nav.clone(), action(move |_, _, _| client.stop_game()), cx)),
        )
    }

    fn render_hintbar(&self) -> Div {
        let mode = self.nav.borrow().mode;
        let mouse = mode == InputMode::Mouse;
        // Buttons of the controller in use, or the keyboard keys.
        let pad = (mode == InputMode::Gamepad).then_some(self.pad_kind);
        let hint = |k: Vec<PadPrompt>, label: &str| {
            let mut d = div().flex().items_center().gap(r(6.));
            for p in k {
                d = d.children(prompt(pad, p, 32., text()));
            }
            d.child(label.to_string())
        };
        let mut bar = div()
            .flex()
            .items_center()
            .gap(r(22.))
            .px(r(24.))
            .h(r(44.))
            .flex_none()
            .text_size(r(13.5))
            .text_color(muted())
            .border_t_1()
            .border_color(line())
            .bg(bg())
            .opacity(if mouse { 0.5 } else { 1.0 })
            .child(hint(vec![PadPrompt::Move], "Move"))
            .child(hint(vec![PadPrompt::Confirm], "Select"))
            .child(hint(vec![PadPrompt::Back], "Back"));
        if (self.tab == Tab::Vault && self.store_sel.is_none()) || self.tab == Tab::Library {
            bar = bar.child(hint(vec![PadPrompt::Search], "Search"));
        }
        bar = bar.child(hint(vec![PadPrompt::TabLeft, PadPrompt::TabRight], "Switch tabs"));
        if pad.is_some() && self.state.session.is_some() {
            bar = bar.child(hint(vec![PadPrompt::Menu], "Settings"));
        }
        bar = bar.child(div().flex_1());
        let pending = self.state.pending_sessions;
        if pending > 0 {
            bar = bar.child(format!("{pending} play session{} waiting to sync", if pending > 1 { "s" } else { "" }));
        }
        bar.child(if self.state.online { "Connected" } else { "Offline" })
    }

    pub fn render_toasts(&self) -> Div {
        let mut d = div().absolute().right(r(24.)).bottom(r(60.)).flex().flex_col().gap(r(8.));
        for t in &self.toasts {
            d = d.child(
                div()
                    .max_w(r(420.))
                    .px(r(16.))
                    .py(r(12.))
                    .rounded(r(RADIUS))
                    .text_size(r(14.))
                    .bg(if t.error { c(0x4a1f22) } else { surface3() })
                    .text_color(if t.error { c(0xffd4d4) } else { text() })
                    .child(t.message.clone()),
            );
        }
        d
    }

    pub fn render_modals(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = self.nav.clone();
        let mut out = Vec::new();
        let count = self.modals.len();
        for idx in 0..count {
            nav.borrow_mut().push_scope();
            // The body's items belong to the "modal" group (initial focus); the close button does not.
            group_begin(&nav, "modal", false);
            let (title, wide, body) = self.modal_body(idx, window, cx);
            nav.borrow_mut().pop_group();
            let close = icon_btn(format!("modal:{idx}:close"), "x", &nav, action(|this, window, cx| this.close_modal(window, cx)), cx);
            let mut m = div()
                .id(SharedString::from(format!("modal-{idx}")))
                .flex()
                .flex_col()
                .w(r(if wide { 1100. } else { 520. }))
                .max_w(relative(0.95))
                .max_h(relative(0.92))
                .overflow_hidden()
                .bg(surface())
                .border_1()
                .border_color(line())
                .rounded(r(12.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            if let Some(t) = title {
                m = m.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pl(r(24.))
                        .pr(r(18.))
                        .pt(r(18.))
                        .child(div().text_size(r(17.)).font_weight(FontWeight::SEMIBOLD).child(t))
                        .child(close),
                );
            }
            let content = with_bounds(div().flex().flex_col().gap(r(14.)).px(r(24.)).pt(r(18.)).pb(r(24.)).min_h_0().child(body), &nav, "g:modal".into());
            m = m.child(content);
            nav.borrow_mut().pop_scope();
            out.push(
                div()
                    .id(SharedString::from(format!("backdrop-{idx}")))
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(ca(0x000000b3))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_modal(window, cx)))
                    .child(m)
                    .into_any_element(),
            );
        }
        out
    }

    /// (title, wide, body) of the modal at `idx`.
    fn modal_body(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) -> (Option<String>, bool, AnyElement) {
        let nav = self.nav.clone();
        let _ = window;
        let list_handle = self.scroll("modal-list");
        let versions_handle = self.scroll("versions");
        match &self.modals[idx].0 {
            Modal::Select { title, options, selected, on_pick } => {
                let title = title.clone();
                let handle = list_handle;
                scroll_begin(&nav, "modal-list", &handle);
                let mut list = div().flex().flex_col().gap(r(2.));
                for (i, o) in options.iter().enumerate() {
                    let id: SharedString = format!("opt:{idx}:{i}").into();
                    let focused = nav.borrow().ring(&id);
                    let sel = i == *selected;
                    let mut row = div()
                        .id(id.clone())
                        .flex()
                        .items_center()
                        .gap(r(12.))
                        .min_h(r(46.))
                        .px(r(14.))
                        .py(r(8.))
                        .rounded(r(RADIUS))
                        .when(sel || focused, |d| d.bg(if focused { surface3() } else { surface2() }))
                        .hover(|s| s.bg(surface2()))
                        .child(div().flex_1().child(o.label.clone()));
                    if let Some(h) = &o.hint {
                        row = row.child(div().text_size(r(13.)).text_color(muted()).child(h.clone()));
                    }
                    if sel {
                        row = row.child(icon("check", 18., play()));
                    }
                    let pick = on_pick.clone();
                    let act = action(move |this, window, cx| {
                        this.close_modal(window, cx);
                        pick(this, i, window, cx);
                    });
                    list = list.child(focusable_ext(row, &nav, id, Ring::Inset(RADIUS), Some(act), false, sel, cx));
                }
                (Some(title), false, scroll_end(&nav, "modal-list", &handle, list).max_h(r(560.)).into_any_element())
            }
            Modal::MultiSelect { title, key, values, labels } => {
                // Several values: choosing an option toggles it and the dialog stays open; the
                // vault behind it updates right away. The first option ("All ...") clears the filter.
                let (title, key) = (title.clone(), *key);
                let chosen = self.prefs.vault_filters.list(key).clone();
                let first_checked = values.iter().position(|v| if v.is_empty() { chosen.is_empty() } else { chosen.contains(v) }).unwrap_or(0);
                let handle = list_handle;
                scroll_begin(&nav, "modal-list", &handle);
                let mut list = div().flex().flex_col().gap(r(2.));
                for (i, (value, label)) in values.iter().zip(labels.iter()).enumerate() {
                    let id: SharedString = format!("opt:{idx}:{i}").into();
                    let focused = nav.borrow().ring(&id);
                    let on = if value.is_empty() { chosen.is_empty() } else { chosen.contains(value) };
                    let check = div()
                        .size(r(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(r(5.))
                        .when(on, |d| d.bg(play()))
                        .when(!on, |d| d.border(r(1.5)).border_color(faint()))
                        .when(on, |d| d.child(icon("check", 14., play_ink())));
                    let row = div()
                        .id(id.clone())
                        .flex()
                        .items_center()
                        .gap(r(12.))
                        .min_h(r(46.))
                        .px(r(14.))
                        .py(r(8.))
                        .rounded(r(RADIUS))
                        .when(focused, |d| d.bg(surface3()))
                        .hover(|s| s.bg(surface2()))
                        .child(check)
                        .child(div().flex_1().child(label.clone()));
                    let v = value.clone();
                    let act = action(move |this, _, cx| {
                        this.prefs.vault_filters.toggle(key, &v);
                        this.save_prefs();
                        cx.notify();
                    });
                    list = list.child(focusable_ext(row, &nav, id, Ring::Inset(RADIUS), Some(act), false, i == first_checked, cx));
                }
                let list = scroll_end(&nav, "modal-list", &handle, list).max_h(r(520.));
                let count = chosen.len();
                let foot = div()
                    .flex()
                    .items_center()
                    .gap(r(10.))
                    .child(div().flex_1().text_size(r(13.)).text_color(muted()).child(match count {
                        0 => "Nothing selected: all are shown".to_string(),
                        1 => "1 selected".to_string(),
                        n => format!("{n} selected"),
                    }))
                    .child(btn(format!("multi:{idx}:done"), "Done").primary().render(&nav, action(|this, window, cx| this.close_modal(window, cx)), cx));
                (Some(title), false, div().flex().flex_col().gap(r(14.)).child(list).child(foot).into_any_element())
            }
            Modal::Confirm { title, message, confirm, danger, on_confirm } => {
                let title = title.clone();
                let on = on_confirm.clone();
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(r(14.))
                    .child(div().text_color(text2()).line_height(r(22.)).child(message.clone()))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(r(10.))
                            .child(btn(format!("confirm:{idx}:cancel"), "Cancel").render(&nav, action(|this, window, cx| this.close_modal(window, cx)), cx))
                            .child(
                                btn(format!("confirm:{idx}:ok"), confirm.clone())
                                    .kind(if *danger { BtnKind::DangerPrimary } else { BtnKind::Primary })
                                    .autofocus()
                                    .render(&nav, action(move |this, window, cx| {
                                        this.close_modal(window, cx);
                                        on(this, window, cx);
                                    }), cx),
                            ),
                    );
                (Some(title), false, body.into_any_element())
            }
            Modal::InstallTo { kind, id } => {
                let (kind, id) = (*kind, *id);
                let s = self.client.settings();
                let def = s.default_location();
                let mut list = div().flex().flex_col().gap(r(2.));
                for (i, l) in s.locations.iter().enumerate() {
                    let oid: SharedString = format!("loc:{idx}:{i}").into();
                    let focused = nav.borrow().ring(&oid);
                    let row = div()
                        .id(oid.clone())
                        .flex()
                        .items_center()
                        .gap(r(12.))
                        .min_h(r(46.))
                        .px(r(14.))
                        .rounded(r(RADIUS))
                        .when(focused, |d| d.bg(surface3()))
                        .hover(|s| s.bg(surface2()))
                        .child(icon("folder", 18., muted()))
                        .child(div().flex_1().child(l.clone()));
                    let loc = l.clone();
                    let client = self.client.clone();
                    let act = action(move |this, window, cx| {
                        this.close_modal(window, cx);
                        let c = client.clone();
                        let loc = loc.clone();
                        this.act(async move { c.install(kind, id, Some(loc)).await }, None, cx);
                    });
                    list = list.child(focusable_ext(row, &nav, oid, Ring::Inset(RADIUS), Some(act), false, def.as_deref() == Some(l.as_str()), cx));
                }
                (Some("Install to…".into()), false, list.into_any_element())
            }
            Modal::Lightbox { game_id, shots, index } => {
                let (gid, n, i) = (*game_id, shots.len(), *index);
                let path = self.client.media(gid, &shots[i]);
                let image = match path {
                    Some(p) => img(p).max_h(r(640.)).w_full().object_fit(ObjectFit::Contain).rounded(r(RADIUS)).into_any_element(),
                    None => div().h(r(400.)).w_full().into_any_element(),
                };
                let step = move |d: isize| {
                    action(move |this, _, cx| {
                        if let Some((Modal::Lightbox { index, .. }, _)) = this.modals.last_mut() {
                            *index = ((*index as isize + d).rem_euclid(n as isize)) as usize;
                        }
                        cx.notify();
                    })
                };
                let body = div()
                    .flex()
                    .items_center()
                    .gap(r(10.))
                    .pt(r(24.))
                    .child(icon_btn(format!("lb:{idx}:prev"), "back", &nav, step(-1), cx))
                    .child(div().flex_1().min_w_0().child(image))
                    .child(focusable_ext(
                        div().id(SharedString::from(format!("lb:{idx}:next"))).flex().items_center().justify_center().size(r(40.)).rounded(r(RADIUS)).hover(|s| s.bg(surface2())).child(icon("forward", 18., muted())),
                        &nav,
                        format!("lb:{idx}:next").into(),
                        Ring::Outer(RADIUS),
                        Some(step(1)),
                        false,
                        true,
                        cx,
                    ));
                (None, true, body.into_any_element())
            }
            Modal::SaveVersions { emulator_id, versions } => {
                let emu_id = *emulator_id;
                let emu_name = self.state.emulator(emu_id).map(|e| e.name.clone()).unwrap_or_default();
                let installed = self.state.installed(ItemKind::Emulator, emu_id).is_some();
                let running = self.state.running.as_ref().is_some_and(|r| r.emulator_id == emu_id);
                let on_device = self.state.save_sync.get(&emu_id).and_then(|s| s.0.version_id);
                let mut body = div().flex().flex_col().gap(r(14.));
                match versions {
                    None => body = body.child(div().text_color(muted()).child("Loading…")),
                    Some(v) if v.is_empty() => body = body.child(div().text_color(muted()).child("No cloud saves yet.")),
                    Some(v) => {
                        let handle = versions_handle;
                        scroll_begin(&nav, "versions", &handle);
                        let mut list = div().flex().flex_col().gap(r(2.));
                        for (i, ver) in v.iter().enumerate() {
                            let id: SharedString = format!("ver:{idx}:{}", ver.id).into();
                            let focused = nav.borrow().ring(&id);
                            let disabled = !installed || running;
                            let mut row = div()
                                .id(id.clone())
                                .flex()
                                .items_center()
                                .gap(r(12.))
                                .min_h(r(46.))
                                .px(r(14.))
                                .rounded(r(RADIUS))
                                .when(focused, |d| d.bg(surface3()))
                                .when(disabled, |d| d.opacity(0.55))
                                .hover(|s| s.bg(surface2()))
                                .child(div().w(r(60.)).font_weight(FontWeight::BOLD).child(format!("v{}", ver.version)))
                                .child(div().w(r(180.)).child(fmt_date(Some(&ver.created_at))))
                                .child(div().flex_1().text_color(muted()).child(ver.device.clone().unwrap_or_default()))
                                .child(div().flex_1().text_color(muted()).child(ver.note.clone().unwrap_or_default()))
                                .child(div().w(r(80.)).text_color(muted()).child(fmt_bytes(ver.size as u64)));
                            if i == 0 {
                                row = row.child(badge("newest", play(), play_ink()));
                            }
                            if Some(ver.id) == on_device {
                                row = row.child(badge("on device", info(), c(0x06121e)));
                            }
                            let v2 = ver.clone();
                            let client = self.client.clone();
                            let name = emu_name.clone();
                            let act = (!disabled).then(|| {
                                action(move |this, window, cx| {
                                    let v3 = v2.clone();
                                    let client = client.clone();
                                    let on_confirm = action(move |this, window, cx| {
                                        // Close the version list too.
                                        this.modals.clear();
                                        this.request_focus(FocusTarget::First, window, cx);
                                        let c = client.clone();
                                        let vid = v3.id;
                                        this.act(async move { c.restore_save(emu_id, vid).await }, None, cx);
                                    });
                                    this.open_modal(
                                        Modal::Confirm {
                                            title: format!("Restore v{}?", v2.version),
                                            message: format!(
                                                "Your current local {name} saves are backed up and replaced by version {} from {}.",
                                                v2.version,
                                                fmt_date(Some(&v2.created_at))
                                            ),
                                            confirm: "Restore".into(),
                                            danger: false,
                                            on_confirm,
                                        },
                                        window,
                                        cx,
                                    );
                                })
                            });
                            list = list.child(focusable_ext(row, &nav, id, Ring::Inset(RADIUS), act, false, i == 0, cx));
                        }
                        body = body.child(scroll_end(&nav, "versions", &handle, list).max_h(r(480.)));
                    }
                }
                let client = self.client.clone();
                body = body
                    .child(div().text_size(r(13.)).text_color(muted()).child(
                        "Restoring copies the version to this device (your current local saves are backed up first) and uploads it as the newest version. No cloud save is ever deleted.",
                    ))
                    .child(div().flex().child(btn(format!("ver:{idx}:backups"), "Open local backups").ghost().icon("folder").render(
                        &nav,
                        action(move |_, _, cx| {
                            let dir = client.paths.save_backup_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            cx.open_with_system(&dir);
                        }),
                        cx,
                    )));
                (Some(format!("Cloud saves – {emu_name}")), true, body.into_any_element())
            }
        }
    }
}
