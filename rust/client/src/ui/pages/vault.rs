//! The vault: filters and the game grid (port of Vault.tsx). The store page opens on top.

use super::{AnyItem, text_input, view_width};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::{AppView, Filters, Modal, PickFn, SelectOption};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::{ItemKind, locale_compare};
use std::collections::BTreeSet;
use std::rc::Rc;

fn uniq(xs: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = xs.filter(|s| !s.is_empty()).collect::<BTreeSet<_>>().into_iter().collect();
    v.sort_by(|a, b| locale_compare(a, b));
    v
}

fn decade(y: Option<i64>) -> String {
    y.map(|y| format!("{}s", y / 10 * 10)).unwrap_or_default()
}

impl AppView {
    pub fn render_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some((kind, id)) = self.store_sel {
            return self.render_store(kind, id, window, cx).into_any_element();
        }
        let nav = self.nav.clone();
        let state = self.state.clone();
        let f = self.prefs.vault_filters.clone();
        let q = self.input_value(&self.inputs.vault_search.clone(), cx).trim().to_lowercase();
        let games = &state.catalog_games;
        let emus = &state.catalog_emulators;

        // Options.
        let mut plats: Vec<String> = games.iter().map(|g| g.platform.clone()).collect();
        if f.show_emulators {
            plats.extend(emus.iter().map(|e| e.platform.clone()));
        }
        let platforms: Vec<(String, String)> = uniq(plats.into_iter())
            .into_iter()
            .map(|p| {
                let name = games.iter().find(|g| g.platform == p).map(|g| g.platform_name.clone()).or_else(|| emus.iter().find(|e| e.platform == p).map(|e| e.platform_name.clone())).unwrap_or(p.clone());
                (p, name)
            })
            .collect();
        let genres = uniq(games.iter().flat_map(|g| g.genres.clone()));
        let themes = uniq(games.iter().flat_map(|g| g.themes.clone()));
        let modes = uniq(games.iter().flat_map(|g| g.game_modes.clone()));
        let devs = uniq(games.iter().flat_map(|g| g.developers.clone()));
        let decades = uniq(games.iter().map(|g| decade(g.year)));

        // Items.
        let mut items: Vec<AnyItem> = games.iter().cloned().map(AnyItem::Game).collect();
        if f.show_emulators {
            items.extend(emus.iter().cloned().map(AnyItem::Emu));
        }
        items.retain(|it| {
            if !q.is_empty() && !it.name().to_lowercase().contains(&q) && !it.platform_name().to_lowercase().contains(&q) {
                return false;
            }
            if !f.platform.is_empty() && it.platform() != f.platform {
                return false;
            }
            if f.installed_only && state.installed(it.kind(), it.id()).is_none() {
                return false;
            }
            if f.library_only && !state.in_library(it.kind(), it.id()) {
                return false;
            }
            match it {
                AnyItem::Game(g) => {
                    (f.genre.is_empty() || g.genres.contains(&f.genre))
                        && (f.theme.is_empty() || g.themes.contains(&f.theme))
                        && (f.mode.is_empty() || g.game_modes.contains(&f.mode))
                        && (f.developer.is_empty() || g.developers.contains(&f.developer) || g.publishers.contains(&f.developer))
                        && (f.decade.is_empty() || decade(g.year) == f.decade)
                }
                AnyItem::Emu(_) => f.genre.is_empty() && f.theme.is_empty() && f.mode.is_empty() && f.developer.is_empty() && f.decade.is_empty(),
            }
        });
        items.sort_by(|a, b| {
            let by = |x: &AnyItem| x.name().to_lowercase();
            if a.kind() != b.kind() {
                return if a.kind() == ItemKind::Game { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
            }
            match (f.sort.as_str(), a, b) {
                ("year", AnyItem::Game(x), AnyItem::Game(y)) => y.year.unwrap_or(0).cmp(&x.year.unwrap_or(0)).then_with(|| locale_compare(&by(a), &by(b))),
                ("rating", AnyItem::Game(x), AnyItem::Game(y)) => {
                    y.rating.unwrap_or(0.0).total_cmp(&x.rating.unwrap_or(0.0)).then_with(|| locale_compare(&by(a), &by(b)))
                }
                ("added", _, _) => b.added_at().cmp(a.added_at()),
                _ => locale_compare(&by(a), &by(b)),
            }
        });
        if !state.online {
            items.sort_by_key(|it| state.installed(it.kind(), it.id()).is_none());
        }

        let width = view_width(window, &state);
        let filters_w = if width < 1100. { 230. } else { 280. };

        // ---- filter column
        let active_filters = [&f.platform, &f.genre, &f.theme, &f.mode, &f.developer, &f.decade].iter().filter(|s| !s.is_empty()).count()
            + f.installed_only as usize
            + f.library_only as usize;
        let filter_handle = self.scroll("vault-filters");
        let all = |l: &str| ("".to_string(), l.to_string());
        let mk_opts = |first: (String, String), rest: Vec<(String, String)>| {
            let mut v = vec![first];
            v.extend(rest);
            v
        };
        let simple = |xs: &Vec<String>| xs.iter().map(|x| (x.clone(), x.clone())).collect::<Vec<_>>();
        let selects: Vec<(&str, &str, Vec<(String, String)>, String)> = vec![
            ("platform", "Platform", mk_opts(all("All platforms"), platforms), f.platform.clone()),
            ("genre", "Genre", mk_opts(all("Any genre"), simple(&genres)), f.genre.clone()),
            ("theme", "Theme", mk_opts(all("Any theme"), simple(&themes)), f.theme.clone()),
            ("mode", "Game mode", mk_opts(all("Any mode"), simple(&modes)), f.mode.clone()),
            ("decade", "Era", mk_opts(all("Any year"), simple(&decades)), f.decade.clone()),
            ("developer", "Developer / publisher", mk_opts(all("Anyone"), simple(&devs)), f.developer.clone()),
            (
                "sort",
                "Sort by",
                vec![("name".into(), "Name".into()), ("year".into(), "Release year".into()), ("rating".into(), "Rating".into()), ("added".into(), "Recently added".into())],
                if f.sort.is_empty() { "name".into() } else { f.sort.clone() },
            ),
        ];
        let filters_el = group(&nav, "filters", true, || {
            let mut col = div().flex().flex_col().gap(r(8.)).px(r(16.)).py(r(24.)).child(text_input(&nav, "vault:search", &self.inputs.vault_search, true, cx));
            for (key, label, opts, value) in selects {
                let current = opts.iter().find(|(v, _)| *v == value).map(|(_, l)| l.clone()).unwrap_or_else(|| "–".into());
                let selected = opts.iter().position(|(v, _)| *v == value).unwrap_or(0);
                let values: Vec<String> = opts.iter().map(|(v, _)| v.clone()).collect();
                let options: Vec<SelectOption> = opts.into_iter().map(|(_, l)| SelectOption { label: l, hint: None }).collect();
                let title = label.to_string();
                let key2 = key.to_string();
                let on_pick: PickFn = Rc::new(move |this: &mut AppView, i, _, cx| {
                    let v = values[i].clone();
                    let f = &mut this.prefs.vault_filters;
                    match key2.as_str() {
                        "platform" => f.platform = v,
                        "genre" => f.genre = v,
                        "theme" => f.theme = v,
                        "mode" => f.mode = v,
                        "decade" => f.decade = v,
                        "developer" => f.developer = v,
                        _ => f.sort = v,
                    }
                    this.save_prefs();
                    cx.notify();
                });
                let options = Rc::new(options);
                col = col.child(select(
                    format!("filter:{key}"),
                    label,
                    &current,
                    &nav,
                    action(move |this, window, cx| {
                        let opts: Vec<SelectOption> = options.iter().map(|o| SelectOption { label: o.label.clone(), hint: o.hint.clone() }).collect();
                        this.open_modal(Modal::Select { title: title.clone(), options: opts, selected, on_pick: on_pick.clone() }, window, cx);
                    }),
                    cx,
                ));
            }
            let toggles: [(&str, &str, bool, fn(&mut Filters)); 3] = [
                ("emus", "Show emulators", f.show_emulators, |f| f.show_emulators = !f.show_emulators),
                ("installed", "Installed only", f.installed_only, |f| f.installed_only = !f.installed_only),
                ("library", "In my library only", f.library_only, |f| f.library_only = !f.library_only),
            ];
            let mut tg = div().flex().flex_col().gap(r(2.)).mt(r(8.));
            for (key, label, on, flip) in toggles {
                tg = tg.child(toggle(format!("filter:{key}"), label, None, on, &nav, action(move |this, _, cx| {
                    flip(&mut this.prefs.vault_filters);
                    this.save_prefs();
                    cx.notify();
                }), cx));
            }
            col = col.child(tg);
            if active_filters > 0 {
                col = col.child(btn("filter:clear", format!("Clear {active_filters} filter{}", if active_filters > 1 { "s" } else { "" })).ghost().render(
                    &nav,
                    action(|this, _, cx| {
                        let f = &this.prefs.vault_filters;
                        this.prefs.vault_filters = Filters { show_emulators: f.show_emulators, sort: f.sort.clone(), ..Default::default() };
                        this.save_prefs();
                        cx.notify();
                    }),
                    cx,
                ));
            }
            div().h_full().child(scroll_area(&nav, "vault-filters", &filter_handle, || col).h_full())
        })
        .w(r(filters_w))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(line());

        // ---- grid
        let main_w = width - filters_w - 64. - 12.;
        let gap = 20.;
        let cols = ((main_w + gap) / (170. + gap)).floor().max(1.);
        let card_w = (main_w - gap * (cols - 1.)) / cols;
        let main_handle = self.scroll("vault-main");
        let count = items.len();
        let mut grid = div().flex().flex_wrap().gap_x(r(gap)).gap_y(r(26.));
        for (i, it) in items.iter().enumerate() {
            grid = grid.child(self.game_card(it, card_w, i == 0, cx));
        }
        let offline = (!state.online).then(|| self.offline_banner(cx));
        let empty_msg = if !games.is_empty() {
            "Nothing matches these filters."
        } else if state.online {
            "The server has no games yet."
        } else {
            "No cached catalog available offline."
        };
        let main_el = group(&nav, "vault-main", true, || {
            let content = div()
                .px(r(32.))
                .pt(r(24.))
                .pb(r(48.))
                .children(offline)
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(r(14.))
                        .mb(r(22.))
                        .child(h1("Vault"))
                        .child(div().pb(r(4.)).text_color(muted()).child(format!("{count} {}", if count == 1 { "title" } else { "titles" }))),
                )
                .child(if count > 0 { grid.into_any_element() } else { div().py(r(60.)).text_center().text_color(muted()).child(empty_msg).into_any_element() });
            div().size_full().child(scroll_area(&nav, "vault-main", &main_handle, || content).size_full())
        })
        .flex_1()
        .min_w_0()
        .h_full();

        div().flex().size_full().child(filters_el).child(main_el).into_any_element()
    }
}
