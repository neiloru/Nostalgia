//! Store page of a game or emulator (port of StorePage.tsx).

use super::{AnyItem, item_status, view_width};
use crate::ui::theme::surface;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::{AppView, Modal};
use crate::util::{fmt_bytes, fmt_duration, fmt_long_date};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::{GameDetail, ItemKind, locale_compare};

/// Hero image of a game: first artwork, else first screenshot.
pub fn hero_name(d: Option<&GameDetail>) -> Option<String> {
    let m = d?.metadata_typed()?;
    m.artworks.first().or(m.screenshots.first()).cloned()
}

fn fact(label: &str, value: Option<String>) -> Option<Div> {
    let v = value.filter(|v| !v.is_empty())?;
    Some(div().flex().gap(r(12.)).child(div().w(r(110.)).flex_none().text_color(muted()).child(label.to_string())).child(div().flex_1().min_w_0().text_color(text2()).child(v)))
}

impl AppView {
    pub fn render_store(&mut self, kind: ItemKind, id: i64, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let handle = self.scroll("store");
        // The page is built between begin and end so its items belong to the group and scroll area.
        group_begin(&nav, "store", false);
        scroll_begin(&nav, "store", &handle);
        let content = match kind {
            ItemKind::Game => self.store_game(id, window, cx),
            ItemKind::Emulator => self.store_emulator(id, window, cx),
        };
        let area = scroll_end(&nav, "store", &handle, content).size_full();
        group_end(&nav, "store", div().size_full().child(area)).size_full()
    }

    fn back_button(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        btn("store:back", "Vault").icon("back").render(&self.nav.clone(), action(|this, window, cx| this.close_store(window, cx)), cx)
    }

    fn unavailable(&mut self, what: &str, cx: &mut Context<Self>) -> Div {
        div().p(r(32.)).flex().flex_col().gap(r(20.)).child(div().child(self.back_button(cx))).child(div().py(r(60.)).text_center().text_color(muted()).child(what.to_string()))
    }

    fn store_game(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let detail = self.detail(id, cx);
        let summary = state.game(id).cloned().or_else(|| detail.as_ref().map(|d| d.summary.clone()));
        let Some(g) = summary else { return self.unavailable("This game is not available.", cx) };
        let m = detail.as_ref().and_then(|d| d.metadata_typed());
        let jobs = self.jobs.clone();
        let st = item_status(&state, &jobs, ItemKind::Game, id);
        let in_library = st.in_library;
        let installed_path = state.installed(ItemKind::Game, id).map(|i| i.path.clone());
        let unavailable = !state.online && installed_path.is_none();
        let pt = state.playtime.get(&id).cloned();
        let width = view_width(window, &state);
        let narrow = width < 1100.;

        // ---- hero
        let hero_img = hero_name(detail.as_ref()).and_then(|n| self.client.media(id, &n));
        let back = self.back_button(cx);
        let cover = self.cover(&AnyItem::Game(g.clone()), false, RADIUS);
        let mut meta = div().flex().flex_wrap().items_center().gap(r(12.)).text_size(r(14.5)).text_color(text2()).child(div().font_weight(FontWeight::SEMIBOLD).child(g.platform_name.clone()));
        let dot = || div().text_color(faint()).child("·");
        if let Some(d) = m.as_ref().and_then(|m| m.release_date.clone()) {
            meta = meta.child(dot()).child(fmt_long_date(&d));
        }
        if let Some(d) = m.as_ref().and_then(|m| m.developers.first().cloned()) {
            meta = meta.child(dot()).child(d);
        }
        if let Some(rt) = g.rating {
            meta = meta.child(dot()).child(div().flex().items_center().gap(r(5.)).text_color(warn()).font_weight(FontWeight::SEMIBOLD).child(icon("star", 14., warn())).child(format!("{}", rt.round() as i64)));
        }
        let genres = m.as_ref().map(|m| m.genres.clone()).unwrap_or(g.genres.clone());
        let actions = if unavailable {
            div().text_color(muted()).child("Not available while offline.")
        } else {
            self.action_buttons(ItemKind::Game, id, g.size_bytes as u64, true, in_library, cx)
        };
        let mut info = div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .min_w_0()
            .child(div().text_size(r(if narrow { 30. } else { 40. })).font_weight(FontWeight::SEMIBOLD).line_height(r(if narrow { 34. } else { 44. })).child(g.name.clone()))
            .child(meta)
            .child(div().flex().flex_wrap().gap(r(6.)).children(genres.into_iter().map(chip)))
            .child(actions);
        if let Some(p) = &pt {
            info = info.child(div().text_size(r(13.)).text_color(muted()).child(format!("You played {}", fmt_duration(p.seconds))));
        }
        let hero = self.hero(hero_img, back, cover, info, narrow, 440.);

        // ---- body
        group_begin(&nav, "store-main", false);
        let mut main = div().flex().flex_col().flex_1().min_w_0();
        let section = |title: &str, body: AnyElement| div().flex().flex_col().mb(r(34.)).child(h2(title.to_string())).child(body);
        let prose = |t: String| div().max_w(r(720.)).line_height(r(26.)).text_color(text2()).child(t).into_any_element();
        if let Some(s) = m.as_ref().and_then(|m| m.summary.clone()) {
            main = main.child(section("About", prose(s)));
        }
        if let Some(s) = m.as_ref().and_then(|m| m.storyline.clone()) {
            main = main.child(section("Story", prose(s)));
        }
        let shots: Vec<String> = m.as_ref().map(|m| m.screenshots.iter().chain(m.artworks.iter()).cloned().collect()).unwrap_or_default();
        if !shots.is_empty() {
            let main_w = if narrow { width - 80. } else { width - 80. - 320. - 40. };
            let cols = ((main_w + 12.) / (240. + 12.)).floor().max(1.);
            let shot_w = (main_w - 12. * (cols - 1.)) / cols;
            let mut grid = div().flex().flex_wrap().gap(r(12.));
            for (i, s) in shots.iter().enumerate() {
                let sid: SharedString = format!("shot:{i}").into();
                let image = match self.client.media(id, s) {
                    Some(p) => img(p).size_full().rounded(r(RADIUS)).object_fit(ObjectFit::Cover).into_any_element(),
                    None => div().size_full().into_any_element(),
                };
                let tile = div().id(sid.clone()).w(r(shot_w)).h(r(shot_w * 9. / 16.)).rounded(r(RADIUS)).overflow_hidden().bg(surface2()).child(image);
                let all = shots.clone();
                grid = grid.child(focusable(
                    tile,
                    &nav,
                    sid,
                    Ring::Outer(RADIUS),
                    Some(action(move |this, window, cx| this.open_modal(Modal::Lightbox { game_id: id, shots: all.clone(), index: i }, window, cx))),
                    cx,
                ));
            }
            main = main.child(section("Media", grid.into_any_element()));
        }
        if m.is_none() {
            main = main.child(div().text_color(muted()).child(if state.online { "No details from IGDB yet." } else { "Details are not cached on this device." }));
        }
        let main_group = group_end(&nav, "store-main", main);

        let join = |v: Option<&Vec<String>>| v.filter(|v| !v.is_empty()).map(|v| v.join(", "));
        let facts = div()
            .flex()
            .flex_col()
            .gap(r(10.))
            .py(r(4.))
            .text_size(r(14.))
            .children(fact("Platform", Some(g.platform_name.clone())))
            .children(fact("Developer", join(m.as_ref().map(|m| &m.developers))))
            .children(fact("Publisher", join(m.as_ref().map(|m| &m.publishers))))
            .children(fact("Release", m.as_ref().and_then(|m| m.release_date.clone())))
            .children(fact("Genres", join(m.as_ref().map(|m| &m.genres))))
            .children(fact("Themes", join(m.as_ref().map(|m| &m.themes))))
            .children(fact("Modes", join(m.as_ref().map(|m| &m.game_modes))))
            .children(fact("Perspective", join(m.as_ref().map(|m| &m.perspectives))))
            .children(fact("Series", join(m.as_ref().map(|m| &m.franchises))))
            .children(fact("Size", Some(format!("{} · {} files", fmt_bytes(g.size_bytes as u64), g.file_count))))
            .children(fact("Installed in", installed_path));
        let emus: Vec<_> = state.catalog.emulators.iter().filter(|e| e.platform == g.platform).cloned().collect();
        let side = group(&nav, "store-side", false, || {
            let mut side = div().flex().flex_col().gap(r(18.)).w(if narrow { relative(1.) } else { r(320.).into() }).flex_none().child(facts);
            if !emus.is_empty() {
                let mut bx = div().flex().flex_col().gap(r(4.)).pt(r(16.)).border_t_1().border_color(line()).child(div().mb(r(6.)).text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(muted()).child("Runs with"));
                for e in &emus {
                    let eid = e.id;
                    let lid: SharedString = format!("store:emu:{eid}").into();
                    let focused = nav.borrow().ring(&lid);
                    let mut row = div()
                        .id(lid.clone())
                        .flex()
                        .items_center()
                        .gap(r(10.))
                        .min_h(r(42.))
                        .px(r(12.))
                        .rounded(r(RADIUS))
                        .text_color(text2())
                        .when(focused, |d| d.bg(surface3()))
                        .hover(|s| s.bg(surface2()).text_color(text()))
                        .child(icon("chip", 16., muted()))
                        .child(e.name.clone());
                    if state.installed(ItemKind::Emulator, eid).is_some() {
                        row = row.child(badge("installed", play(), play_ink()));
                    }
                    bx = bx.child(focusable(row, &nav, lid, Ring::Inset(RADIUS), Some(action(move |this, window, cx| this.open_store(ItemKind::Emulator, eid, window, cx))), cx));
                }
                side = side.child(bx);
            }
            if let Some(url) = m.as_ref().and_then(|m| m.url.clone()) {
                side = side.child(btn("store:igdb", "View on IGDB").ghost().render(&nav, action(move |_, _, cx| cx.open_url(&url)), cx));
            }
            side
        });
        let body = div()
            .flex()
            .when(narrow, |d| d.flex_col())
            .gap(r(40.))
            .px(r(40.))
            .pt(r(12.))
            .pb(r(60.))
            .child(main_group.flex_1().min_w_0())
            .child(side);
        div().flex().flex_col().child(hero).child(body)
    }

    /// Hero header with background image, cover and info (store pages).
    fn hero(&mut self, image: Option<crate::client::Media>, back: Stateful<Div>, cover: AnyElement, info: Div, narrow: bool, min_h: f32) -> Div {
        let mut hero = div().relative().flex().items_end().min_h(r(min_h)).bg(surface()).overflow_hidden();
        if let Some(p) = image {
            hero = hero.child(img(p).absolute().inset_0().size_full().object_fit(ObjectFit::Cover));
        }
        hero.child(div().absolute().inset_0().bg(linear_gradient(0., linear_color_stop(bg(), 0.), linear_color_stop(ca(0x0f0f1173), 1.))))
            .child(div().absolute().inset_0().bg(linear_gradient(0., linear_color_stop(ca(0x0f0f11e0), 0.4), linear_color_stop(ca(0x0f0f1100), 1.))))
            .child(div().absolute().top(r(20.)).left(r(32.)).child(back))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_end()
                    .gap(r(32.))
                    .px(r(40.))
                    .pt(r(84.))
                    .pb(r(32.))
                    .w_full()
                    .child(div().w(r(if narrow { 160. } else { 210. })).h(r(if narrow { 213. } else { 280. })).flex_none().rounded(r(RADIUS)).overflow_hidden().child(cover))
                    .child(info.flex_1()),
            )
    }

    fn store_emulator(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let Some(e) = state.emulator(id).cloned() else { return self.unavailable("This emulator is not available.", cx) };
        let jobs = self.jobs.clone();
        let st = item_status(&state, &jobs, ItemKind::Emulator, id);
        let installed_path = state.installed(ItemKind::Emulator, id).map(|i| i.path.clone());
        let os = state.os.unwrap_or(nostalgia_shared::OsKey::current());
        let sys = e.config.as_ref().and_then(|c| c.systems.get(os).cloned());
        let latest = state.save_sync.get(&id).and_then(|s| s.2.clone());
        let width = view_width(window, &state);
        let narrow = width < 1100.;

        let back = self.back_button(cx);
        let cover = self.cover(&AnyItem::Emu(e.clone()), false, RADIUS);
        let supports = e.supports.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(" · ");
        let mut info = div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .min_w_0()
            .child(div().text_size(r(if narrow { 30. } else { 40. })).font_weight(FontWeight::SEMIBOLD).child(e.name.clone()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(r(12.))
                    .text_size(r(14.5))
                    .text_color(text2())
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(e.platform_name.clone()))
                    .child(div().text_color(faint()).child("·"))
                    .child("Emulator")
                    .child(div().text_color(faint()).child("·"))
                    .child(if supports.is_empty() { "no configuration".to_string() } else { supports.clone() }),
            );
        if let Some(d) = e.config.as_ref().and_then(|c| c.description.clone()) {
            info = info.child(div().line_height(r(26.)).text_color(text2()).child(d));
        }
        if sys.is_none() {
            info = info.child(div().text_size(r(13.5)).text_color(err_soft()).child(format!("This emulator has no configuration for {}.", os.as_str())));
        }
        info = info.child(if !state.online && installed_path.is_none() {
            div().text_color(muted()).child("Not available while offline.")
        } else {
            self.action_buttons(ItemKind::Emulator, id, e.size_bytes as u64, true, st.in_library, cx)
        });
        let hero = self.hero(None, back, cover, info, narrow, 320.);

        let mut games: Vec<_> = state.catalog.games.iter().filter(|g| g.platform == e.platform).collect();
        games.sort_by(|a, b| locale_compare(&a.name, &b.name));
        let total = games.len();
        games.truncate(super::LIST_MAX);
        let main = group(&nav, "store-main", false, || {
            let mut saves = div()
                .flex()
                .flex_col()
                .mb(r(34.))
                .child(h2("Cloud saves"))
                .child(div().text_color(muted()).line_height(r(26.)).child(format!(
                    "Saves are synchronised per emulator. {}",
                    match &latest {
                        Some(v) => format!("Your newest cloud save is version {}.", v.version),
                        None => "You have no cloud saves for this emulator yet.".into(),
                    }
                )));
            for s in sys.as_ref().and_then(|s| s.saves.clone()).unwrap_or_default() {
                saves = saves.child(div().font_family(MONO).text_size(r(12.5)).text_color(text2()).child(format!("• {s}")));
            }
            let mut list = div().flex().flex_col().gap(r(2.));
            for g in games {
                let gid = g.id;
                let lid: SharedString = format!("store:game:{gid}").into();
                let focused = nav.borrow().ring(&lid);
                let row = div()
                    .id(lid.clone())
                    .flex()
                    .items_center()
                    .min_h(r(42.))
                    .px(r(12.))
                    .rounded(r(RADIUS))
                    .text_color(text2())
                    .when(focused, |d| d.bg(surface3()))
                    .hover(|s| s.bg(surface2()).text_color(text()))
                    .child(g.name.clone());
                list = list.child(focusable(row, &nav, lid, Ring::Inset(RADIUS), Some(action(move |this, window, cx| this.open_store(ItemKind::Game, gid, window, cx))), cx));
            }
            if total == 0 {
                list = list.child(div().text_color(muted()).child("No games for this platform yet."));
            } else if total > super::LIST_MAX {
                let platform = e.platform.clone();
                list = list.child(div().mt(r(10.)).child(btn("store:all-games", format!("Show all {total} games in the vault")).render(
                    &nav,
                    action(move |this, window, cx| this.show_platform_in_vault(&platform, false, window, cx)),
                    cx,
                )));
            }
            div().flex().flex_col().child(saves).child(div().flex().flex_col().child(h2(format!("Games for {}", e.platform_name))).child(list))
        });
        let facts = div()
            .flex()
            .flex_col()
            .gap(r(10.))
            .text_size(r(14.))
            .w(if narrow { relative(1.) } else { r(320.).into() })
            .children(fact("Platform", Some(e.platform_name.clone())))
            .children(fact("Version", e.config.as_ref().and_then(|c| c.version.clone())))
            .children(fact("Size", Some(format!("{} · {} files", fmt_bytes(e.size_bytes as u64), e.file_count))))
            .children(fact("Systems", Some(e.supports.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", "))))
            .children(fact("Installed in", installed_path));
        let body = div().flex().when(narrow, |d| d.flex_col()).gap(r(40.)).px(r(40.)).pt(r(12.)).pb(r(60.)).child(main.flex_1().min_w_0()).child(facts);
        div().flex().flex_col().child(hero).child(body)
    }
}
