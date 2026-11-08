//! Sign in / create account (port of Login.tsx).

use super::{field, text_input};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::AppView;
use gpui_kit::*;

impl AppView {
    pub fn render_login(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let register = self.register;
        let mut form = div()
            .flex()
            .flex_col()
            .gap(r(14.))
            .w(r(400.))
            .child(div().text_size(r(32.)).font_weight(FontWeight::SEMIBOLD).child("Nostalgia"))
            .child(div().mb(r(10.)).text_color(muted()).child(if register { "Create your account" } else { "Sign in to your game server" }))
            .child(field("Server", text_input(&nav, "login:server", &self.inputs.server, false, cx)))
            .child(field("Username", text_input(&nav, "login:username", &self.inputs.username, false, cx)))
            .child(field("Password", text_input(&nav, "login:password", &self.inputs.password, false, cx)));
        if register {
            form = form.child(field("Repeat password", text_input(&nav, "login:password2", &self.inputs.password2, false, cx)));
        }
        if !self.login_error.is_empty() {
            form = form.child(div().text_size(r(13.5)).text_color(err_soft()).child(self.login_error.clone()));
        }
        let label = if self.login_busy {
            "Please wait…"
        } else if register {
            "Create account"
        } else {
            "Sign in"
        };
        form = form
            .child(btn("login:submit", label).big().primary().full().disabled(self.login_busy).render(&nav, action(|this, window, cx| this.submit_login(window, cx)), cx))
            .child(
                btn("login:toggle", if register { "I already have an account" } else { "Create a new account" })
                    .ghost()
                    .full()
                    .render(&nav, action(|this, _, cx| {
                        this.register = !this.register;
                        this.login_error.clear();
                        cx.notify();
                    }), cx),
            )
            .child(div().mt(r(6.)).text_size(r(12.)).text_color(muted()).text_center().child(format!("v{} · {}", self.state.version, self.state.device)));
        div().size_full().flex().items_center().justify_center().bg(bg()).child(form)
    }

    pub fn submit_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.login_busy {
            return;
        }
        let server = self.input_value(&self.inputs.server.clone(), cx);
        let user = self.input_value(&self.inputs.username.clone(), cx);
        let pass = self.input_value(&self.inputs.password.clone(), cx);
        let pass2 = self.input_value(&self.inputs.password2.clone(), cx);
        self.login_error.clear();
        if user.is_empty() || pass.is_empty() || server.is_empty() {
            self.login_error = "Please fill in all fields".into();
            cx.notify();
            return;
        }
        if self.register && pass != pass2 {
            self.login_error = "Passwords do not match".into();
            cx.notify();
            return;
        }
        self.login_busy = true;
        let register = self.register;
        let client = self.client.clone();
        let task = self.client.rt.spawn({
            let (server, user) = (server.clone(), user.clone());
            async move { client.login(&server, &user, &pass, register).await }
        });
        let _ = window;
        cx.spawn(async move |this, cx| {
            let r = task.await;
            let _ = this.update(cx, |this, cx| {
                this.login_busy = false;
                match r {
                    Ok(Ok(())) => {
                        this.prefs.last_server = Some(server);
                        this.prefs.last_user = Some(user);
                        this.save_prefs();
                    }
                    Ok(Err(e)) => this.login_error = e.message,
                    Err(e) => this.login_error = e.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
