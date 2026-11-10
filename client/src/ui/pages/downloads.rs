//! Download queue (port of Downloads.tsx).

use super::AnyItem;
use crate::model::{DownloadJob, DownloadStatus};
use crate::ui::AppView;
use crate::ui::theme::surface;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::util::{fmt_bytes, fmt_date};
use gpui_kit::*;
use nostalgia_shared::ItemKind;

fn eta(j: &DownloadJob) -> String {
    if j.speed <= 0.0 {
        return String::new();
    }
    let s = (j.total_bytes.saturating_sub(j.received_bytes)) as f64 / j.speed;
    if s > 3600.0 {
        format!("{} h {} min left", (s / 3600.0).floor(), ((s % 3600.0) / 60.0).round())
    } else if s > 60.0 {
        format!("{} min left", (s / 60.0).round())
    } else {
        format!("{} s left", s.round())
    }
}

fn status_label(s: DownloadStatus) -> &'static str {
    match s {
        DownloadStatus::Queued => "Queued",
        DownloadStatus::Downloading => "Downloading",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::Error => "Failed",
        DownloadStatus::Done => "Installed",
    }
}

impl AppView {
    pub fn render_downloads(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        // Everything on the page is built inside the scroll area (focused items scroll into view).
        let handle = self.scroll("downloads");
        scroll_begin(&nav, "downloads", &handle);
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let active: Vec<&DownloadJob> = jobs.iter().filter(|j| j.status != DownloadStatus::Done).collect();
        let mut done: Vec<&DownloadJob> = jobs.iter().filter(|j| j.status == DownloadStatus::Done).collect();
        done.sort_by(|a, b| b.finished_at.cmp(&a.finished_at));
        let running = active.iter().any(|j| matches!(j.status, DownloadStatus::Downloading | DownloadStatus::Queued));
        let paused = active.iter().any(|j| j.status == DownloadStatus::Paused);
        let client = self.client.clone();

        let mut head = div().flex().items_center().gap(r(14.)).mb(r(22.)).child(h1("Downloads")).child(div().flex_1());
        if running {
            let c = client.clone();
            head = head.child(btn("dl:pause-all", "Pause all").icon("pause").render(&nav, action(move |_, _, _| c.pause_all_downloads()), cx));
        }
        if paused {
            let c = client.clone();
            head = head.child(btn("dl:resume-all", "Resume all").icon("download").disabled(!state.online).render(&nav, action(move |_, _, _| c.resume_all_downloads()), cx));
        }
        if !done.is_empty() {
            let c = client.clone();
            head = head.child(btn("dl:clear", "Clear finished").ghost().render(&nav, action(move |_, _, _| c.clear_finished_downloads()), cx));
        }

        let mut page = div().flex().flex_col().px(r(32.)).pt(r(24.)).pb(r(50.));
        if !state.online {
            page = page.child(self.offline_banner(cx));
        }
        page = page.child(head);
        if active.is_empty() && done.is_empty() {
            page = page.child(div().py(r(60.)).text_center().text_color(muted()).child("No downloads. Install games from the vault or your library."));
        }
        let mut list = div().flex().flex_col().gap(r(8.));
        for j in &active {
            let item = match j.kind {
                ItemKind::Game => state.game(j.id).cloned().map(AnyItem::Game),
                ItemKind::Emulator => state.emulator(j.id).cloned().map(AnyItem::Emu),
            };
            let thumb = match &item {
                Some(it) => self.cover(it, true, 6., 54.),
                None => div().into_any_element(),
            };
            let status_color = match j.status {
                DownloadStatus::Downloading => info(),
                DownloadStatus::Error => err(),
                DownloadStatus::Paused => warn(),
                _ => muted(),
            };
            let mut meta = div().flex().flex_wrap().gap(r(16.)).text_size(r(13.)).text_color(muted()).child(format!("{} / {}", fmt_bytes(j.received_bytes), fmt_bytes(j.total_bytes)));
            if j.status == DownloadStatus::Downloading {
                meta = meta.child(format!("{}/s", fmt_bytes(j.speed as u64))).child(eta(j));
                if let Some(f) = &j.current_file {
                    meta = meta.child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(f.clone()));
                }
            }
            if let Some(e) = &j.error {
                meta = meta.child(div().text_color(err_soft()).child(e.clone()));
            }
            let key = j.key.clone();
            let c = client.clone();
            let toggle_btn = if matches!(j.status, DownloadStatus::Downloading | DownloadStatus::Queued) {
                let k = key.clone();
                btn(format!("dl:{key}:pause"), "Pause").icon("pause").render(&nav, action(move |_, _, _| c.pause_download(&k)), cx)
            } else {
                let k = key.clone();
                btn(format!("dl:{key}:resume"), "Resume").primary().icon("download").disabled(!state.online).render(&nav, action(move |_, _, _| c.resume_download(&k)), cx)
            };
            let c = client.clone();
            let k = key.clone();
            let cancel = btn(format!("dl:{key}:cancel"), "Cancel").ghost().icon("x").render(&nav, action(move |_, _, _| c.cancel_download(&k)), cx);
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(18.))
                    .p(r(14.))
                    .rounded(r(10.))
                    .bg(surface())
                    .child(div().w(r(54.)).h(r(72.)).flex_none().rounded(r(6.)).overflow_hidden().bg(surface2()).child(thumb))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(r(8.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_end()
                                    .gap(r(12.))
                                    .child(div().font_weight(FontWeight::BOLD).child(j.name.clone()))
                                    .child(div().text_color(muted()).child(format!("{}{}", if j.kind == ItemKind::Emulator { "Emulator · " } else { "" }, j.platform.to_uppercase())))
                                    .child(div().ml_auto().text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(status_color).child(status_label(j.status))),
                            )
                            .child(progress(j.received_bytes, j.total_bytes, j.status != DownloadStatus::Downloading))
                            .child(meta)
                            .child(div().text_size(r(13.)).text_color(muted()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(j.target_dir.clone())),
                    )
                    .child(div().flex().gap(r(8.)).child(toggle_btn).child(cancel)),
            );
        }
        page = page.child(list);
        if !done.is_empty() {
            let mut d = div().flex().flex_col().child(div().mt(r(34.)).child(h2("Completed").text_color(muted())));
            for j in done {
                d = d.child(
                    div()
                        .flex()
                        .items_end()
                        .gap(r(12.))
                        .px(r(16.))
                        .py(r(12.))
                        .border_b_1()
                        .border_color(line())
                        .child(div().font_weight(FontWeight::BOLD).child(j.name.clone()))
                        .child(div().text_color(muted()).child(fmt_bytes(j.total_bytes)))
                        .child(div().text_color(muted()).child(fmt_date(j.finished_at.as_deref()))),
                );
            }
            page = page.child(d);
        }
        div().size_full().child(scroll_end(&nav, "downloads", &handle, page).size_full())
    }
}
