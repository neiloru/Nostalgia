//! The vault: filters and the game grid (port of Vault.tsx). The store page opens on top.

use super::{AnyItem, text_input, view_width};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::{AppView, Filters, Modal, PickFn, SelectOption};
use crate::model::{Catalog, ClientState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::{GameSummary, ItemKind, LibraryState, locale_compare};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

fn uniq(xs: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = xs.filter(|s| !s.is_empty()).collect::<BTreeSet<_>>().into_iter().collect();
    v.sort_by(|a, b| locale_compare(a, b));
    v
}

fn decade(y: Option<i64>) -> String {
    y.map(|y| format!("{}s", y / 10 * 10)).unwrap_or_default()
}

/// A vault entry: index into the catalog's games or emulators.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    Game(usize),
    Emu(usize),
}

impl Entry {
    fn item(self, c: &Catalog) -> AnyItem {
        match self {
            Entry::Game(i) => AnyItem::Game(c.games[i].clone()),
            Entry::Emu(i) => AnyItem::Emu(c.emulators[i].clone()),
        }
    }
}

/// What the filter column offers.
#[derive(Default)]
struct Options {
    platforms: Vec<(String, String)>,
    genres: Vec<String>,
    themes: Vec<String>,
    modes: Vec<String>,
    devs: Vec<String>,
    decades: Vec<String>,
}

impl Options {
    fn new(c: &Catalog, show_emulators: bool) -> Options {
        let games = &c.games;
        let mut platforms: BTreeMap<String, String> = BTreeMap::new();
        for g in games.iter() {
            platforms.entry(g.platform.clone()).or_insert_with(|| g.platform_name.clone());
        }
        if show_emulators {
            for e in c.emulators.iter() {
                platforms.entry(e.platform.clone()).or_insert_with(|| e.platform_name.clone());
            }
        }
        let mut platforms: Vec<(String, String)> = platforms.into_iter().filter(|(p, _)| !p.is_empty()).collect();
        platforms.sort_by(|a, b| locale_compare(&a.0, &b.0));
        Options {
            platforms,
            genres: uniq(games.iter().flat_map(|g| g.genres.iter().cloned())),
            themes: uniq(games.iter().flat_map(|g| g.themes.iter().cloned())),
            modes: uniq(games.iter().flat_map(|g| g.game_modes.iter().cloned())),
            devs: uniq(games.iter().flat_map(|g| g.developers.iter().cloned())),
            decades: uniq(games.iter().map(|g| decade(g.year))),
        }
    }
}

/// What the shown items depend on besides the catalog.
#[derive(Clone, PartialEq)]
struct ItemsKey {
    filters: Filters,
    query: String,
    online: bool,
    installed: (Vec<i64>, Vec<i64>),
    library: LibraryState,
}

/// The vault's filter options and its filtered, sorted items. The catalog can hold thousands of
/// games, so they are computed again only when the catalog, the filters or the search change.
#[derive(Default)]
pub struct VaultMemo {
    catalog: Option<Arc<Catalog>>,
    show_emulators: bool,
    options: Rc<Options>,
    /// Lower-case names of the games and emulators (search, sorting).
    names: (Vec<String>, Vec<String>),
    /// Every entry in the order of a sort key.
    sorted: Option<(String, Rc<Vec<Entry>>)>,
    key: Option<ItemsKey>,
    items: Rc<Vec<Entry>>,
    /// Filters and search the items were last shown for.
    shown_for: Option<(Filters, String)>,
}

impl VaultMemo {
    /// Brings options and items up to date; whether other filters or another search are shown
    /// than last time.
    fn update(&mut self, state: &ClientState, f: &Filters, query: &str) -> bool {
        let shown_for = Some((f.clone(), query.to_string()));
        let refiltered = self.shown_for.is_some() && self.shown_for != shown_for;
        self.shown_for = shown_for;
        let c = &state.catalog;
        let same_catalog = self.catalog.as_ref().is_some_and(|x| Arc::ptr_eq(x, c));
        if !same_catalog || self.show_emulators != f.show_emulators {
            if !same_catalog {
                self.names = (c.games.iter().map(|g| g.name.to_lowercase()).collect(), c.emulators.iter().map(|e| e.name.to_lowercase()).collect());
            }
            self.catalog = Some(c.clone());
            self.show_emulators = f.show_emulators;
            self.options = Rc::new(Options::new(c, f.show_emulators));
            self.sorted = None;
            self.key = None;
        }
        let sort = if f.sort.is_empty() { "name" } else { f.sort.as_str() };
        if self.sorted.as_ref().is_none_or(|(s, _)| s != sort) {
            self.sorted = Some((sort.to_string(), Rc::new(self.sort(c, sort))));
        }
        let key = ItemsKey {
            filters: f.clone(),
            query: query.to_string(),
            online: state.online,
            installed: (state.installed.games.keys().copied().collect(), state.installed.emulators.keys().copied().collect()),
            library: state.library.clone(),
        };
        if self.key.as_ref() == Some(&key) {
            return refiltered;
        }
        let all = self.sorted.as_ref().map(|(_, v)| v.clone()).unwrap_or_default();
        let mut items: Vec<Entry> = all.iter().copied().filter(|e| self.matches(state, *e, f, query)).collect();
        if !state.online {
            items.sort_by_key(|e| match *e {
                Entry::Game(i) => state.installed(ItemKind::Game, c.games[i].id).is_none(),
                Entry::Emu(i) => state.installed(ItemKind::Emulator, c.emulators[i].id).is_none(),
            });
        }
        self.items = Rc::new(items);
        self.key = Some(key);
        refiltered
    }

    /// Games first, then emulators (when shown), each by the sort key and then by name.
    fn sort(&self, c: &Catalog, sort: &str) -> Vec<Entry> {
        let (gn, en) = &self.names;
        let g = |i: usize| -> &GameSummary { &c.games[i] };
        let mut games: Vec<usize> = (0..c.games.len()).collect();
        games.sort_by(|&a, &b| {
            let by_name = || locale_compare(&gn[a], &gn[b]);
            match sort {
                "year" => g(b).year.unwrap_or(0).cmp(&g(a).year.unwrap_or(0)).then_with(by_name),
                "rating" => g(b).rating.unwrap_or(0.0).total_cmp(&g(a).rating.unwrap_or(0.0)).then_with(by_name),
                "added" => g(b).added_at.cmp(&g(a).added_at),
                _ => by_name(),
            }
        });
        let mut emus: Vec<usize> = if self.show_emulators { (0..c.emulators.len()).collect() } else { Vec::new() };
        emus.sort_by(|&a, &b| match sort {
            "added" => c.emulators[b].added_at.cmp(&c.emulators[a].added_at),
            _ => locale_compare(&en[a], &en[b]),
        });
        games.into_iter().map(Entry::Game).chain(emus.into_iter().map(Entry::Emu)).collect()
    }

    fn matches(&self, state: &ClientState, e: Entry, f: &Filters, q: &str) -> bool {
        let c = &state.catalog;
        let (kind, id, platform, platform_name, name) = match e {
            Entry::Game(i) => (ItemKind::Game, c.games[i].id, &c.games[i].platform, &c.games[i].platform_name, &self.names.0[i]),
            Entry::Emu(i) => (ItemKind::Emulator, c.emulators[i].id, &c.emulators[i].platform, &c.emulators[i].platform_name, &self.names.1[i]),
        };
        if !q.is_empty() && !name.contains(q) && !platform_name.to_lowercase().contains(q) {
            return false;
        }
        if !f.platform.is_empty() && !f.platform.iter().any(|p| p == platform) {
            return false;
        }
        if f.installed_only && state.installed(kind, id).is_none() {
            return false;
        }
        if f.library_only && !state.in_library(kind, id) {
            return false;
        }
        // A filter with several values matches any of them.
        let any = |list: &Vec<String>, hit: &dyn Fn(&String) -> bool| list.is_empty() || list.iter().any(hit);
        match e {
            Entry::Game(i) => {
                let g = &c.games[i];
                any(&f.genre, &|v| g.genres.contains(v))
                    && any(&f.theme, &|v| g.themes.contains(v))
                    && any(&f.mode, &|v| g.game_modes.contains(v))
                    && any(&f.developer, &|v| g.developers.contains(v) || g.publishers.contains(v))
                    && any(&f.decade, &|v| decade(g.year) == *v)
            }
            Entry::Emu(_) => f.genre.is_empty() && f.theme.is_empty() && f.mode.is_empty() && f.developer.is_empty() && f.decade.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, VaultMemo};
    use crate::model::{Catalog, ClientState};
    use crate::ui::Filters;
    use nostalgia_shared::GameSummary;
    use std::rc::Rc;
    use std::sync::Arc;

    fn game(id: i64, name: &str, platform: &str, year: i64) -> GameSummary {
        serde_json::from_value(serde_json::json!({ "id": id, "kind": "game", "name": name, "platform": platform, "platformName": platform.to_uppercase(),
            "folder": "f", "igdbId": 0, "sizeBytes": 0, "fileCount": 0, "hasCover": false, "hasArtwork": false, "year": year, "addedAt": "" }))
        .unwrap()
    }

    fn state(games: Vec<GameSummary>) -> ClientState {
        ClientState { online: true, catalog: Arc::new(Catalog::new(Arc::new(games), Arc::new(Vec::new()))), ..Default::default() }
    }

    fn names(m: &VaultMemo, s: &ClientState) -> Vec<String> {
        m.items.iter().map(|e| match e {
            Entry::Game(i) => s.catalog.games[*i].name.clone(),
            Entry::Emu(i) => s.catalog.emulators[*i].name.clone(),
        }).collect()
    }

    #[test]
    fn items_are_sorted_filtered_and_kept_until_something_changes() {
        let s = state(vec![game(1, "Zelda", "snes", 1991), game(2, "ape escape", "psx", 1999), game(3, "Bomberman", "snes", 1993)]);
        let mut m = VaultMemo::default();
        let mut f = Filters::default();
        assert!(!m.update(&s, &f, ""), "the first list is not a change of filters");
        assert_eq!(names(&m, &s), ["ape escape", "Bomberman", "Zelda"]);
        let before = m.items.clone();
        assert!(!m.update(&s, &f, ""));
        assert!(Rc::ptr_eq(&before, &m.items), "nothing is computed again");

        assert!(m.update(&s, &f, "ze"));
        assert_eq!(names(&m, &s), ["Zelda"]);
        f.platform = vec!["snes".into()];
        f.sort = "year".into();
        assert!(m.update(&s, &f, ""));
        assert_eq!(names(&m, &s), ["Bomberman", "Zelda"]);

        // A new catalog from the server is not a change of filters (the scroll position stays).
        let s2 = state(vec![game(1, "Zelda", "snes", 1991), game(4, "Contra III", "snes", 1992)]);
        assert!(!m.update(&s2, &f, ""));
        assert_eq!(names(&m, &s2), ["Contra III", "Zelda"]);
    }

    #[test]
    fn the_catalog_finds_games_by_id() {
        let s = state(vec![game(7, "A", "psx", 2000), game(3, "B", "psx", 2001)]);
        assert_eq!(s.game(3).map(|g| g.name.as_str()), Some("B"));
        assert!(s.game(5).is_none());
    }
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
        let refiltered = self.vault.update(&state, &f, &q);
        let (options, items) = (self.vault.options.clone(), self.vault.items.clone());
        let catalog = state.catalog.clone();
        let main_handle = self.scroll("vault-main");
        if refiltered {
            // Start at the top of the new list.
            main_handle.set_offset(point(px(0.), px(0.)));
        }

        let width = view_width(window, &state);
        let filters_w = if width < 1100. { 230. } else { 280. };

        // ---- filter column
        let active_filters = Filters::LISTS.iter().filter(|k| !f.list(k).is_empty()).count()
            + f.installed_only as usize
            + f.library_only as usize;
        let filter_handle = self.scroll("vault-filters");
        let simple = |xs: &Vec<String>| xs.iter().map(|x| (x.clone(), x.clone())).collect::<Vec<_>>();
        // (key, label, "all" label, (value, label) options): several values can be picked.
        let lists: Vec<(&'static str, &str, &str, Vec<(String, String)>)> = vec![
            ("platform", "Platform", "All platforms", options.platforms.clone()),
            ("genre", "Genre", "Any genre", simple(&options.genres)),
            ("theme", "Theme", "Any theme", simple(&options.themes)),
            ("mode", "Game mode", "Any mode", simple(&options.modes)),
            ("decade", "Era", "Any year", simple(&options.decades)),
            ("developer", "Developer / publisher", "Anyone", simple(&options.devs)),
        ];
        let sort_opts: Vec<(String, String)> =
            vec![("name".into(), "Name".into()), ("year".into(), "Release year".into()), ("rating".into(), "Rating".into()), ("added".into(), "Recently added".into())];
        let sort_value = if f.sort.is_empty() { "name".to_string() } else { f.sort.clone() };
        let filters_el = group(&nav, "filters", true, || {
            scroll_begin(&nav, "vault-filters", &filter_handle);
            let mut col = div().flex().flex_col().gap(r(8.)).px(r(16.)).py(r(24.)).child(text_input(&nav, "vault:search", &self.inputs.vault_search, true, cx));
            for (key, label, all_label, opts) in lists {
                // "Any genre", "Action", "Action +2"
                let chosen: Vec<String> =
                    f.list(key).iter().map(|v| opts.iter().find(|(o, _)| o == v).map(|(_, l)| l.clone()).unwrap_or_else(|| v.clone())).collect();
                let current = match chosen.len() {
                    0 => all_label.to_string(),
                    1 => chosen[0].clone(),
                    n => format!("{} +{}", chosen[0], n - 1),
                };
                let mut values = vec![String::new()];
                let mut labels = vec![all_label.to_string()];
                for (v, l) in opts {
                    values.push(v);
                    labels.push(l);
                }
                let title = label.to_string();
                col = col.child(select(
                    format!("filter:{key}"),
                    label,
                    &current,
                    &nav,
                    action(move |this, window, cx| {
                        this.open_modal(Modal::MultiSelect { title: title.clone(), key, values: values.clone(), labels: labels.clone() }, window, cx);
                    }),
                    cx,
                ));
            }
            {
                let current = sort_opts.iter().find(|(v, _)| *v == sort_value).map(|(_, l)| l.clone()).unwrap_or_else(|| "–".into());
                let selected = sort_opts.iter().position(|(v, _)| *v == sort_value).unwrap_or(0);
                let values: Vec<String> = sort_opts.iter().map(|(v, _)| v.clone()).collect();
                let options: Rc<Vec<SelectOption>> = Rc::new(sort_opts.into_iter().map(|(_, l)| SelectOption { label: l, hint: None }).collect());
                let on_pick: PickFn = Rc::new(move |this: &mut AppView, i, _, cx| {
                    this.prefs.vault_filters.sort = values[i].clone();
                    this.save_prefs();
                    cx.notify();
                });
                col = col.child(select(
                    "filter:sort",
                    "Sort by",
                    &current,
                    &nav,
                    action(move |this, window, cx| {
                        let opts: Vec<SelectOption> = options.iter().map(|o| SelectOption { label: o.label.clone(), hint: o.hint.clone() }).collect();
                        this.open_modal(Modal::Select { title: "Sort by".into(), options: opts, selected, on_pick: on_pick.clone() }, window, cx);
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
            div().h_full().child(scroll_end(&nav, "vault-filters", &filter_handle, col).h_full())
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
        let count = items.len();
        // The cards are built between begin and end so they belong to the group and scroll area
        // (moving the focus scrolls them into view). Only the rows on screen are built: the vault
        // can hold thousands of games.
        group_begin(&nav, "vault-main", true);
        scroll_begin(&nav, "vault-main", &main_handle);
        let cols = cols as usize;
        let stride = super::card_height(card_w) + 26.;
        let rows = count.div_ceil(cols);
        let shown = visible_rows(&main_handle, window, 24. + self.vault_header_h.get(), stride, rows);
        let mut grid = div().flex().flex_col();
        if shown.start > 0 {
            grid = grid.child(div().flex_none().h(r(shown.start as f32 * stride)));
        }
        for row in shown.clone() {
            let mut line = div().flex().flex_none().items_start().gap_x(r(gap)).h(r(stride));
            for (i, e) in items.iter().enumerate().skip(row * cols).take(cols) {
                line = line.child(self.game_card(&e.item(&catalog), card_w, i == 0, cx));
            }
            grid = grid.child(line);
        }
        if shown.end < rows {
            grid = grid.child(div().flex_none().h(r((rows - shown.end) as f32 * stride)));
        }
        let offline = (!state.online).then(|| self.offline_banner(cx));
        let empty_msg = if !catalog.games.is_empty() {
            "Nothing matches these filters."
        } else if state.online {
            "The server has no games yet."
        } else {
            "No cached catalog available offline."
        };
        let main_el = {
            let content = div()
                .px(r(32.))
                .pt(r(24.))
                .pb(r(48.))
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
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
                        .child(measure_height(self.vault_header_h.clone())),
                )
                .child(if count > 0 { grid.into_any_element() } else { div().py(r(60.)).text_center().text_color(muted()).child(empty_msg).into_any_element() });
            let area = scroll_end(&nav, "vault-main", &main_handle, content).size_full();
            group_end(&nav, "vault-main", div().size_full().child(area))
        }
        .flex_1()
        .min_w_0()
        .h_full();

        div().flex().size_full().child(filters_el).child(main_el).into_any_element()
    }
}
