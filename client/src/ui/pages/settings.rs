//! Settings (port of Settings.tsx).

use super::{field, text_input, view_width};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::{AppView, Modal, PickFn, SelectOption};
use crate::util::fmt_bytes;
use gpui_kit::*;
use std::rc::Rc;

impl AppView {
    pub fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        // Everything on the page is built inside the scroll area (focused items scroll into view).
        let handle = self.scroll("settings");
        scroll_begin(&nav, "settings", &handle);
        let state = self.state.clone();
        let s = state.settings.clone();
        let client = self.client.clone();
        let width = view_width(window, &state);
        let cols = if width < 1100. { 1. } else { 2. };
        let panel_w = (width - 64. - 16. * (cols - 1.)) / cols;
        if self.locations.len() != s.locations.len() {
            self.refresh_locations();
        }

        // ---- account
        let account = group(&nav, "panel:account", false, || {
            let user = state.session.as_ref().map(|(_, u)| u.username.clone()).unwrap_or_default();
            let server = state.session.as_ref().map(|(s, _)| s.clone()).unwrap_or_default();
            let mut form = div()
                .flex()
                .flex_col()
                .gap(r(14.))
                .child(field("Current password", text_input(&nav, "settings:oldpw", &self.inputs.old_pw, false, cx)))
                .child(field("New password", text_input(&nav, "settings:newpw", &self.inputs.new_pw, false, cx)))
                .child(field("Repeat new password", text_input(&nav, "settings:newpw2", &self.inputs.new_pw2, false, cx)));
            if !self.pw_error.is_empty() {
                form = form.child(div().text_size(r(13.5)).text_color(err_soft()).child(self.pw_error.clone()));
            }
            let old_empty = self.input_value(&self.inputs.old_pw.clone(), cx).is_empty() || self.input_value(&self.inputs.new_pw.clone(), cx).is_empty();
            form = form.child(btn("settings:changepw", "Change password").primary().full().disabled(!state.online || old_empty).render(&nav, action(|this, window, cx| this.change_password(window, cx)), cx));
            let c = client.clone();
            panel()
                .child(h2("Account"))
                .child(kv(vec![
                    ("User".into(), user),
                    ("Server".into(), server),
                    ("Status".into(), if state.online { "Connected".into() } else { "Offline".into() }),
                    ("Device".into(), state.device.clone()),
                ]))
                .child(h3("Change password"))
                .child(form)
                .child(div().mt(r(18.)).flex().child(btn("settings:logout", "Log out").danger().icon("logout").render(&nav, action(move |this, window, cx| {
                    let c = c.clone();
                    this.open_modal(
                        Modal::Confirm {
                            title: "Log out?".into(),
                            message: "Installed games stay on this device. Running downloads are paused.".into(),
                            confirm: "Log out".into(),
                            danger: true,
                            on_confirm: action(move |this, _, cx| {
                                let c = c.clone();
                                this.act(async move { c.logout().await }, None, cx);
                            }),
                        },
                        window,
                        cx,
                    );
                }), cx)))
        });

        // ---- download locations
        let locations = group(&nav, "panel:locations", false, || {
            let installed = &state.installed;
            let default = s.default_location();
            let mut list = div().flex().flex_col().gap(r(8.)).mt(r(12.)).mb(r(14.));
            for (i, p) in s.locations.iter().enumerate() {
                let info = self.locations.iter().find(|l| &l.path == p);
                let count = installed.games.values().chain(installed.emulators.values()).filter(|it| &it.location == p).count();
                let mut sub = String::new();
                match info {
                    Some(l) if l.free.is_some() => sub.push_str(&format!("{} free", fmt_bytes(l.free.unwrap_or(0)))),
                    Some(l) if !l.exists => sub.push_str("not accessible"),
                    _ => {}
                }
                if count > 0 {
                    sub.push_str(&format!(" · {count} installed"));
                }
                let mut row = div()
                    .flex()
                    .items_center()
                    .gap(r(12.))
                    .px(r(12.))
                    .py(r(10.))
                    .rounded(r(RADIUS))
                    .bg(surface2())
                    .child(icon("folder", 18., muted()))
                    .child(div().flex().flex_col().flex_1().min_w_0().child(div().font_family(MONO).text_size(r(12.5)).child(p.clone())).child(div().text_size(r(12.)).text_color(muted()).child(sub)));
                if default.as_deref() == Some(p.as_str()) {
                    row = row.child(badge("default", play(), play_ink()));
                } else {
                    let (c, p2) = (client.clone(), p.clone());
                    row = row.child(btn(format!("loc:{i}:default"), "Make default").small().render(&nav, action(move |_, _, _| c.update_settings(|s| s.default_location = Some(p2.clone()))), cx));
                }
                let (c, p2) = (client.clone(), p.clone());
                row = row.child(btn(format!("loc:{i}:remove"), "").small().ghost().icon("trash").disabled(s.locations.len() < 2 || count > 0).render(&nav, action(move |this, _, cx| {
                    let c = c.clone();
                    let p = p2.clone();
                    this.act(async move { c.remove_location(&p) }, None, cx);
                }), cx));
                list = list.child(row);
            }
            let c = client.clone();
            panel()
                .child(h2("Download locations"))
                .child(div().text_size(r(13.)).text_color(muted()).child("Games are installed to <location>/games/<platform>/ and emulators to <location>/emulators/<platform>/."))
                .child(list)
                .child(div().flex().child(btn("loc:add", "Add location").icon("plus").render(&nav, action(move |_, _, cx| {
                    let c = c.clone();
                    let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Choose a download location".into()) });
                    cx.spawn(async move |this, cx| {
                        if let Ok(Ok(Some(p))) = paths.await
                            && let Some(p) = p.first()
                        {
                            c.add_location(p.to_string_lossy().into_owned());
                            let _ = this.update(cx, |this, cx| {
                                this.refresh_locations();
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                }), cx)))
        });

        // ---- cloud saves & play
        let play_panel = group(&nav, "panel:play", false, || {
            let flip = |f: fn(&mut crate::model::ClientSettings)| {
                let c = client.clone();
                action(move |_, _, _| c.update_settings(f))
            };
            let c = client.clone();
            let autosync = toggle("set:autosync", "Sync cloud saves automatically", Some("Download newest saves on start, upload after playing".into()), s.auto_sync_saves, &nav, flip(|s| s.auto_sync_saves = !s.auto_sync_saves), cx);
            nav.borrow_mut().set_autofocus("set:autosync");
            panel()
                .child(h2("Cloud saves & play"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(r(2.))
                        .mb(r(12.))
                        .child(autosync)
                        .child(toggle("set:emuwithgame", "Install the emulator together with a game", None, s.install_emulator_with_game, &nav, flip(|s| s.install_emulator_with_game = !s.install_emulator_with_game), cx))
                        .child(toggle("set:minimize", "Minimize Nostalgia while playing", None, s.minimize_on_launch, &nav, flip(|s| s.minimize_on_launch = !s.minimize_on_launch), cx)),
                )
                .child(div().flex().child(btn("set:backups", "Open local save backups").ghost().icon("folder").render(&nav, action(move |_, _, cx| {
                    let dir = c.paths.save_backup_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    cx.open_with_system(&dir);
                }), cx)))
        });

        // ---- display
        let display = group(&nav, "panel:display", false, || {
            let c = client.clone();
            let zoom = state.ui_zoom;
            let steps: [f64; 11] = [0.0, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
            let options: Rc<Vec<(String, Option<String>)>> = Rc::new(
                steps
                    .iter()
                    .map(|z| {
                        if *z == 0.0 {
                            (if s.ui_scale > 0.0 { "Automatic".to_string() } else { format!("Automatic ({} %)", (zoom * 100.0).round()) }, Some("By window size, 1080p = 100 %".to_string()))
                        } else {
                            (format!("{} %", (z * 100.0).round()), None)
                        }
                    })
                    .collect(),
            );
            let selected = steps.iter().position(|z| (z - s.ui_scale).abs() < 0.001).unwrap_or(0);
            let current = options[selected].0.clone();
            let pick: PickFn = Rc::new(move |this, i, window, cx| {
                c.update_settings(|s| s.ui_scale = steps[i]);
                this.apply_zoom(window, cx);
            });
            let c2 = client.clone();
            let controls = kv(vec![
                ("Navigate".into(), "D-pad / left stick · arrow keys · mouse".into()),
                ("Select".into(), "A · Enter".into()),
                ("Back".into(), "B · Esc / Backspace".into()),
                ("Switch tabs".into(), "LB / RB · Q / E".into()),
                ("Search / filter".into(), "Y · F".into()),
                ("Settings".into(), "Start".into()),
            ])
            .text_size(r(13.));
            panel()
                .child(h2("Display"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(r(2.))
                        .mb(r(12.))
                        .child(toggle("set:fullscreen", "Fullscreen", Some("F11".into()), state.fullscreen, &nav, action(|this, window, cx| this.toggle_fullscreen(window, cx)), cx))
                        .child(toggle(
                            "set:startfs",
                            "Start in fullscreen",
                            Some("Recommended for Steam Deck / TV".into()),
                            s.start_fullscreen,
                            &nav,
                            {
                                let c = client.clone();
                                action(move |_, _, _| c.update_settings(|s| s.start_fullscreen = !s.start_fullscreen))
                            },
                            cx,
                        )),
                )
                .child(select("set:uiscale", "UI scale", &current, &nav, action(move |this, window, cx| {
                    let options = options.iter().map(|(l, h)| SelectOption { label: l.clone(), hint: h.clone() }).collect();
                    this.open_modal(Modal::Select { title: "UI scale".into(), options, selected, on_pick: pick.clone() }, window, cx);
                }), cx))
                .child(div().mt(r(6.)).text_size(r(13.)).text_color(muted()).child("Ctrl + / Ctrl − / Ctrl 0 change the scale with a keyboard."))
                .child(h3("Controls"))
                .child(controls)
                .child(h3("About"))
                .child(div().text_size(r(13.)).text_color(muted()).child(format!("Nostalgia v{}", state.version)))
                .child(div().mt(r(8.)).flex().child(btn("set:quit", "Quit Nostalgia").ghost().render(&nav, action(move |_, _, cx| {
                    c2.flush();
                    cx.quit();
                }), cx)))
        });

        let mut grid = div().flex().flex_wrap().items_start().gap(r(16.));
        for p in [account, locations, play_panel, display] {
            grid = grid.child(p.w(r(panel_w)));
        }
        let page = div().flex().flex_col().px(r(32.)).pt(r(24.)).pb(r(50.)).child(h1("Settings").mb(r(22.))).child(grid);
        div().size_full().child(scroll_end(&nav, "settings", &handle, page).size_full())
    }

    fn change_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.input_value(&self.inputs.old_pw.clone(), cx);
        let new = self.input_value(&self.inputs.new_pw.clone(), cx);
        let new2 = self.input_value(&self.inputs.new_pw2.clone(), cx);
        self.pw_error.clear();
        if new != new2 {
            self.pw_error = "The new passwords do not match".into();
            cx.notify();
            return;
        }
        if new.chars().count() < 6 {
            self.pw_error = "At least 6 characters".into();
            cx.notify();
            return;
        }
        let client = self.client.clone();
        let task = self.client.rt.spawn(async move { client.change_password(&old, &new).await });
        let _ = window;
        cx.spawn_in(window, async move |this, cx| {
            let r = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match r {
                    Ok(Ok(())) => {
                        for i in [this.inputs.old_pw.clone(), this.inputs.new_pw.clone(), this.inputs.new_pw2.clone()] {
                            i.update(cx, |s, cx| s.set_value("", window, cx));
                        }
                    }
                    Ok(Err(e)) => this.pw_error = e.message,
                    Err(e) => this.pw_error = e.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
