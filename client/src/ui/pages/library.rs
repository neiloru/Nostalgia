//! The personal library: list on the left, details on the right (port of Library.tsx).

use super::store::hero_name;
use super::{AnyItem, item_status, text_input, view_width};
use crate::ui::theme::surface;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::{AppView, Modal, PickFn, SelectOption};
use crate::util::{fmt_bytes, fmt_duration, fmt_relative};
use crate::model::{Catalog, ClientState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use nostalgia_shared::{ItemKind, LibraryState, OsKey, locale_compare, platform_info};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct ListKey {
    query: String,
    show_emus: bool,
    installed_only: bool,
    online: bool,
    installed: (Vec<i64>, Vec<i64>),
    library: LibraryState,
}

/// The library's sorted, filtered entries. A library can hold thousands of games, so they are
/// computed again only when the library, the catalog or the search change.
#[derive(Default)]
pub struct LibraryMemo {
    catalog: Option<Arc<Catalog>>,
    key: Option<ListKey>,
    items: Rc<Vec<(ItemKind, i64)>>,
    /// "kind:lower-case name" of entries whose name is on several platforms.
    dups: Rc<HashSet<String>>,
}

impl LibraryMemo {
    fn update(&mut self, state: &ClientState, key: ListKey) {
        if self.catalog.as_ref().is_some_and(|c| Arc::ptr_eq(c, &state.catalog)) && self.key.as_ref() == Some(&key) {
            return;
        }
        // Installed items that are no longer in the library (e.g. removed on another device) are
        // listed too, so they can be uninstalled or added again.
        let games = state.library.games.iter().chain(state.installed.games.keys().filter(|id| !state.library_games.contains(id)));
        let emulators = state.library.emulators.iter().chain(state.installed.emulators.keys().filter(|id| !state.library_emulators.contains(id)));
        let mut items: Vec<AnyItem> = games.clone().filter_map(|id| state.game(*id).cloned()).map(AnyItem::Game).collect();
        if key.show_emus {
            items.extend(emulators.clone().filter_map(|id| state.emulator(*id).cloned()).map(AnyItem::Emu));
        }
        let q = &key.query;
        items.retain(|it| (q.is_empty() || it.name().to_lowercase().contains(q)) && (!key.installed_only || state.installed(it.kind(), it.id()).is_some()));
        items.sort_by(|a, b| {
            if a.kind() != b.kind() {
                return if a.kind() == ItemKind::Game { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
            }
            if !state.online {
                let ia = state.installed(a.kind(), a.id()).is_some();
                let ib = state.installed(b.kind(), b.id()).is_some();
                if ia != ib {
                    return ib.cmp(&ia);
                }
            }
            locale_compare(a.name(), b.name())
        });
        // Names that appear on more than one platform get the short platform name appended.
        let mut plats: HashMap<String, HashSet<String>> = HashMap::new();
        for it in games.filter_map(|id| state.game(*id)) {
            plats.entry(format!("game:{}", it.name.to_lowercase())).or_default().insert(it.platform.clone());
        }
        for it in emulators.filter_map(|id| state.emulator(*id)) {
            plats.entry(format!("emulator:{}", it.name.to_lowercase())).or_default().insert(it.platform.clone());
        }
        self.dups = Rc::new(plats.into_iter().filter(|(_, p)| p.len() > 1).map(|(k, _)| k).collect());
        self.items = Rc::new(items.iter().map(|i| (i.kind(), i.id())).collect());
        self.catalog = Some(state.catalog.clone());
        self.key = Some(key);
    }
}

fn stat(label: &str, value: String) -> Div {
    div().flex().flex_col().gap(r(3.)).child(div().text_size(r(12.5)).text_color(muted()).child(label.to_string())).child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(value))
}

impl AppView {
    pub fn render_library(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let q = self.input_value(&self.inputs.lib_search.clone(), cx).trim().to_lowercase();
        let (show_emus, installed_only) = (self.prefs.lib_emulators, self.prefs.lib_installed);

        let key = ListKey {
            query: q,
            show_emus,
            installed_only,
            online: state.online,
            installed: (state.installed.games.keys().copied().collect(), state.installed.emulators.keys().copied().collect()),
            library: state.library.clone(),
        };
        self.library.update(&state, key);
        let (items, dups) = (self.library.items.clone(), self.library.dups.clone());

        // Moving through the list selects the entry (like the onFocus handler). Done before the
        // detail panel is chosen, so it shows the focused entry in this frame and not one step late.
        if let Some(f) = nav.borrow().focused.clone()
            && let Some(rest) = f.strip_prefix("lib:")
            && let Some((k, id)) = rest.split_once(':')
            && let Ok(id) = id.parse::<i64>()
        {
            let kind = if k == "game" { ItemKind::Game } else { ItemKind::Emulator };
            if items.contains(&(kind, id)) {
                self.lib_sel = Some((kind, id));
            }
        }
        let current = match self.lib_sel {
            Some(sel) if items.contains(&sel) => Some(sel),
            _ => items.first().copied(),
        };
        let width = view_width(window, &state);
        let list_w = if width < 1100. { 260. } else { 320. };

        let list_handle = self.scroll("lib-list");
        let list = group(&nav, "lib-list", true, || {
            // Rows are built inside the scroll area (scrolled into view when focused); the search
            // box and toggles above it are not.
            scroll_begin(&nav, "lib-list", &list_handle);
            // Only the rows on screen are built (a library can hold thousands of games). Every
            // row, also the "Emulators" heading, is ROW_H high.
            const ROW_H: f32 = 34.;
            let emus_at = items.iter().position(|(k, _)| *k == ItemKind::Emulator);
            let count = items.len() + emus_at.is_some() as usize;
            let shown = visible_rows(&list_handle, window, 8., ROW_H + 2., count);
            let mut rows = div().flex().flex_col().gap(r(2.)).p(r(8.));
            if shown.start > 0 {
                rows = rows.child(div().flex_none().h(r(shown.start as f32 * (ROW_H + 2.) - 2.)));
            }
            for row in shown.clone() {
                // Row index -> entry, with the heading before the first emulator.
                let i = match emus_at {
                    Some(at) if row == at => {
                        rows = rows.child(
                            div().flex().flex_none().items_end().h(r(ROW_H)).px(r(12.)).pb(r(6.)).text_size(r(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(faint()).child("Emulators"),
                        );
                        continue;
                    }
                    Some(at) if row > at => row - 1,
                    _ => row,
                };
                let (kind, iid) = items[i];
                let Some(it) = (match kind {
                    ItemKind::Game => state.game(iid).cloned().map(AnyItem::Game),
                    ItemKind::Emulator => state.emulator(iid).cloned().map(AnyItem::Emu),
                }) else {
                    continue;
                };
                let it = &it;
                let st = item_status(&state, &jobs, it.kind(), it.id());
                let active = current == Some((it.kind(), it.id()));
                let unavailable = !state.online && !st.installed;
                let dup = dups.contains(&format!("{}:{}", it.kind().as_str(), it.name().to_lowercase()));
                let label = if dup { format!("{} ({})", it.name(), platform_info(it.platform()).short) } else { it.name().to_string() };
                let id: SharedString = format!("lib:{}", it.key()).into();
                let focused = nav.borrow().ring(&id);
                let mut row = div()
                    .id(id.clone())
                    .flex()
                    .items_center()
                    .gap(r(10.))
                    .h(r(ROW_H))
                    .flex_none()
                    .px(r(12.))
                    .rounded(r(RADIUS))
                    .text_color(if active || st.installed || focused { text() } else { muted() })
                    .when(active, |d| d.bg(surface2()))
                    .when(focused, |d| d.bg(surface3()))
                    .when(unavailable, |d| d.opacity(0.35))
                    .hover(|s| s.bg(surface2()).text_color(text()))
                    .child(div().flex_1().min_w_0().text_size(r(14.5)).font_weight(FontWeight::MEDIUM).overflow_hidden().whitespace_nowrap().text_ellipsis().child(label));
                if let Some(j) = st.job {
                    row = row.child(div().text_size(r(12.5)).text_color(info()).child(format!("{}%", super::pct(j))));
                }
                if it.kind() == ItemKind::Game && state.running.as_ref().is_some_and(|r| r.game_id == it.id()) {
                    row = row.child(div().size(r(8.)).rounded_full().bg(play()));
                }
                let (k, iid) = (it.kind(), it.id());
                rows = rows.child(focusable_ext(
                    row,
                    &nav,
                    id,
                    Ring::Inset(RADIUS),
                    Some(action(move |this, _, cx| {
                        this.lib_sel = Some((k, iid));
                        cx.notify();
                    })),
                    false,
                    active,
                    cx,
                ));
            }
            if shown.end < count {
                rows = rows.child(div().flex_none().h(r((count - shown.end) as f32 * (ROW_H + 2.) - 2.)));
            }
            if items.is_empty() {
                let mut e = div().flex().flex_col().items_center().gap(r(14.)).py(r(30.)).px(r(12.)).text_size(r(14.)).text_color(muted()).child(if state.library.games.is_empty() {
                    "Your library is empty. Add games from the vault."
                } else {
                    "Nothing matches."
                });
                if state.library.games.is_empty() {
                    e = e.child(btn("lib:open-vault", "Open the vault").primary().render(&nav, action(|this, window, cx| this.set_tab(crate::ui::Tab::Vault, window, cx)), cx));
                }
                rows = rows.child(e);
            }
            let rows_area = scroll_end(&nav, "lib-list", &list_handle, rows);
            div()
                .flex()
                .flex_col()
                .h_full()
                .child(div().mx(r(14.)).mt(r(20.)).mb(r(8.)).child(text_input(&nav, "lib:search", &self.inputs.lib_search, true, cx)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .mx(r(6.))
                        .px(r(8.))
                        .pb(r(8.))
                        .border_b_1()
                        .border_color(line())
                        .child(toggle("lib:emus", "Show emulators", None, show_emus, &nav, action(|this, _, cx| {
                            this.prefs.lib_emulators = !this.prefs.lib_emulators;
                            this.save_prefs();
                            cx.notify();
                        }), cx))
                        .child(toggle("lib:installed", "Installed only", None, installed_only, &nav, action(|this, _, cx| {
                            this.prefs.lib_installed = !this.prefs.lib_installed;
                            this.save_prefs();
                            cx.notify();
                        }), cx)),
                )
                .child(div().flex_1().min_h_0().child(rows_area.size_full()))
        })
        .w(r(list_w))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(line());

        let detail_handle = self.scroll("lib-detail");
        group_begin(&nav, "lib-detail", false);
        scroll_begin(&nav, "lib-detail", &detail_handle);
        let detail_content = {
            let mut d = div().flex().flex_col();
            if !state.online {
                d = d.child(div().mx(r(32.)).mt(r(20.)).child(self.offline_banner(cx)));
            }
            match current {
                Some((ItemKind::Game, id)) => d.child(self.game_detail_panel(id, width - list_w, window, cx)),
                Some((ItemKind::Emulator, id)) => d.child(self.emulator_panel(id, width - list_w, cx)),
                None => d,
            }
        };
        let detail_area = scroll_end(&nav, "lib-detail", &detail_handle, detail_content).size_full();
        let detail = group_end(&nav, "lib-detail", div().size_full().child(detail_area)).flex_1().min_w_0().h_full();
        div().flex().size_full().child(list).child(detail)
    }

    fn detail_hero(&mut self, item: &AnyItem, image: Option<crate::client::Media>, meta: Div) -> Div {
        let cover = self.cover(item, true, RADIUS);
        let mut hero = div().relative().flex().items_end().h(r(300.)).bg(surface()).overflow_hidden();
        if let Some(p) = image {
            hero = hero.child(img(p).absolute().inset_0().size_full().object_fit(ObjectFit::Cover));
        }
        hero.child(div().absolute().inset_0().bg(linear_gradient(0., linear_color_stop(bg(), 0.), linear_color_stop(ca(0x0f0f1173), 1.))))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_end()
                    .gap(r(24.))
                    .px(r(32.))
                    .pb(r(24.))
                    .child(div().w(r(120.)).h(r(160.)).flex_none().rounded(r(RADIUS)).overflow_hidden().child(cover))
                    .child(div().flex().flex_col().gap(r(8.)).child(div().text_size(r(34.)).font_weight(FontWeight::SEMIBOLD).child(item.name().to_string())).child(meta)),
            )
    }

    fn panels(width: f32, panels: Vec<Div>) -> Div {
        let inner = width - 64.;
        let cols = ((inner + 16.) / (320. + 16.)).floor().clamp(1., panels.len().max(1) as f32);
        let w = (inner - 16. * (cols - 1.)) / cols;
        let mut grid = div().flex().flex_wrap().items_start().gap(r(16.)).px(r(32.)).pt(r(24.)).pb(r(50.));
        for p in panels {
            grid = grid.child(p.w(r(w)));
        }
        grid
    }

    fn game_detail_panel(&mut self, id: i64, width: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let Some(g) = state.game(id).cloned() else { return div() };
        let detail = self.detail(id, cx);
        let installed = state.installed(ItemKind::Game, id).cloned();
        let opts = self.client.launch_options(id);
        let unavailable = !state.online && installed.is_none();
        let pt = state.playtime.get(&id).cloned();
        let emu = opts.emulator_id.and_then(|e| state.emulator(e).cloned());

        // Extra arguments input follows the selected game.
        if self.args_for != Some(id) {
            self.save_extra_args(cx);
            self.args_for = Some(id);
            let v = opts.extra_args.clone().unwrap_or_default();
            self.inputs.extra_args.update(cx, |s, cx| s.set_value(v, window, cx));
        }

        let image = hero_name(detail.as_ref()).and_then(|n| self.client.media(id, &n));
        let mut meta = div().flex().flex_wrap().gap(r(12.)).text_size(r(14.5)).text_color(text2()).child(div().font_weight(FontWeight::SEMIBOLD).child(g.platform_name.clone()));
        if let Some(y) = g.year {
            meta = meta.child(div().text_color(faint()).child("·")).child(y.to_string());
        }
        if let Some(d) = g.developers.first() {
            meta = meta.child(div().text_color(faint()).child("·")).child(d.clone());
        }
        let hero = self.detail_hero(&AnyItem::Game(g.clone()), image, meta);

        let actions = if unavailable {
            div().text_color(muted()).child("This game is not installed and you are offline.")
        } else {
            self.action_buttons(ItemKind::Game, id, g.size_bytes as u64, false, false, cx)
        };
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(r(32.))
            .px(r(32.))
            .py(r(20.))
            .border_b_1()
            .border_color(line())
            .child(actions)
            .child(
                div()
                    .flex()
                    .gap(r(36.))
                    .ml_auto()
                    .child(stat("Play time", fmt_duration(pt.as_ref().map(|p| p.seconds).unwrap_or(0))))
                    .child(stat("Last played", fmt_relative(pt.as_ref().and_then(|p| p.last_played.as_deref()))))
                    .child(stat("Size", fmt_bytes(g.size_bytes as u64))),
            );

        // Cloud saves
        let saves_panel = group(&nav, "panel:saves", false, || {
            let body = match &emu {
                Some(e) => self.save_box(e.id, cx),
                None => div().text_color(muted()).child("No emulator available for this platform."),
            };
            panel().child(h2("Cloud saves").child(div())).child(body)
        });

        // Game settings
        let settings_panel = group(&nav, "panel:settings", false, || {
            let mut form = div().flex().flex_col().gap(r(14.));
            let os = state.os.unwrap_or(OsKey::current());
            let emu_options: Vec<(i64, SelectOption)> = opts
                .emulators
                .iter()
                .map(|e| {
                    let hint = if !e.supported { format!("not available for {}", os.as_str()) } else if e.installed { "installed".into() } else { "not installed".into() };
                    (e.id, SelectOption { label: e.name.clone(), hint: Some(hint) })
                })
                .collect();
            let cur_name = emu.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| "–".into());
            let selected = emu_options.iter().position(|(i, _)| Some(*i) == opts.emulator_id).unwrap_or(0);
            let ids: Vec<i64> = emu_options.iter().map(|(i, _)| *i).collect();
            let labels: Rc<Vec<(String, Option<String>)>> = Rc::new(emu_options.iter().map(|(_, o)| (o.label.clone(), o.hint.clone())).collect());
            let client = self.client.clone();
            let pick: PickFn = Rc::new(move |_, i, _, _| {
                let e = ids[i];
                client.update_game_settings(id, |s| s.emulator_id = Some(e));
            });
            form = form.child(select("game:emulator", "Emulator", &cur_name, &nav, action(move |this, window, cx| {
                let options = labels.iter().map(|(l, h)| SelectOption { label: l.clone(), hint: h.clone() }).collect();
                this.open_modal(Modal::Select { title: "Emulator".into(), options, selected, on_pick: pick.clone() }, window, cx);
            }), cx));
            if opts.discs.len() > 1 {
                let discs = opts.discs.clone();
                let cur = opts.disc.clone().unwrap_or_default();
                let cur_label = discs.iter().find(|(f, _)| *f == cur).map(|(_, l)| l.clone()).unwrap_or(cur.clone());
                let selected = discs.iter().position(|(f, _)| *f == cur).unwrap_or(0);
                let client = self.client.clone();
                let files: Vec<String> = discs.iter().map(|(f, _)| f.clone()).collect();
                let pick: PickFn = Rc::new(move |_, i, _, _| {
                    let d = files[i].clone();
                    client.update_game_settings(id, |s| s.disc = Some(d));
                });
                let disc_opts: Rc<Vec<(String, String)>> = Rc::new(discs.clone());
                form = form.child(select("game:disc", "Disc", &cur_label, &nav, action(move |this, window, cx| {
                    let options = disc_opts.iter().map(|(f, l)| SelectOption { label: l.clone(), hint: Some(f.clone()) }).collect();
                    this.open_modal(Modal::Select { title: "Disc".into(), options, selected, on_pick: pick.clone() }, window, cx);
                }), cx));
            }
            if opts.discs.len() == 1 {
                form = form.child(div().text_size(r(13.)).text_color(muted()).child(format!("Launch file: {}", opts.discs[0].0)));
            }
            if installed.is_none() {
                form = form.child(div().text_size(r(13.)).text_color(muted()).child("Install the game to choose discs."));
            }
            form = form.child(super::field("Extra emulator arguments", text_input(&nav, "lib:args", &self.inputs.extra_args, false, cx)));
            if let Some(e) = &emu
                && state.installed(ItemKind::Emulator, e.id).is_none()
            {
                let (eid, client) = (e.id, self.client.clone());
                form = form.child(div().flex().child(btn("game:install-emu", format!("Install {}", e.name)).icon("download").disabled(!state.online).render(&nav, action(move |this, _, cx| {
                    let c = client.clone();
                    this.act(async move { c.install(ItemKind::Emulator, eid, None).await }, None, cx);
                }), cx)));
            }
            panel()
                .child(h2("Game settings").child(div().text_size(r(14.)).font_weight(FontWeight::NORMAL).text_color(muted()).child("(this device only)")))
                .child(form)
        });

        // Manage
        let manage_panel = group(&nav, "panel:manage", false, || self.manage_panel(ItemKind::Game, id, &g.name, installed.as_ref().map(|i| i.path.clone()), cx));

        div().flex().flex_col().child(hero).child(bar).child(Self::panels(width, vec![saves_panel, settings_panel, manage_panel]))
    }

    fn manage_panel(&mut self, kind: ItemKind, id: i64, name: &str, path: Option<String>, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let mut col = div().flex().flex_col().items_start().gap(r(8.));
        col = col.child(btn(format!("manage:{}:store", kind.as_str()), "Store page").render(&nav, action(move |this, window, cx| this.open_store(kind, id, window, cx)), cx));
        if path.is_some() {
            let client = self.client.clone();
            col = col.child(btn(format!("manage:{}:folder", kind.as_str()), "Open folder").icon("folder").render(&nav, action(move |_, _, cx| {
                if let Some(it) = client.installed(kind, id) {
                    cx.open_with_system(std::path::Path::new(&it.path));
                }
            }), cx));
            let running = match kind {
                ItemKind::Game => state.running.as_ref().is_some_and(|r| r.game_id == id),
                ItemKind::Emulator => state.running.as_ref().is_some_and(|r| r.emulator_id == id),
            };
            let client = self.client.clone();
            let name2 = name.to_string();
            col = col.child(btn(format!("manage:{}:uninstall", kind.as_str()), "Uninstall").danger().icon("trash").disabled(running).render(&nav, action(move |this, window, cx| {
                let c = client.clone();
                let message = match kind {
                    ItemKind::Game => "The game files are removed from this device. Your cloud saves and play time stay on the server.",
                    ItemKind::Emulator => "Local saves are uploaded first. Save folders outside of the emulator folder are not touched.",
                };
                this.open_modal(
                    Modal::Confirm {
                        title: format!("Uninstall {name2}?"),
                        message: message.into(),
                        confirm: "Uninstall".into(),
                        danger: true,
                        on_confirm: action(move |this, _, cx| {
                            let c = c.clone();
                            this.act(async move { c.uninstall(kind, id).await }, None, cx);
                        }),
                    },
                    window,
                    cx,
                );
            }), cx));
        }
        let mut p = panel().child(h2("Manage"));
        if state.in_library(kind, id) {
            let installed = path.is_some();
            col = col.child(btn(format!("manage:{}:remove", kind.as_str()), "Remove from library").ghost().disabled(!state.online).render(
                &nav,
                action(move |this, window, cx| this.confirm_remove_from_library(kind, id, installed, window, cx)),
                cx,
            ));
        } else {
            p = p.child(div().mb(r(12.)).text_size(r(13.5)).text_color(muted()).child("Installed on this device, but not in your library."));
            let client = self.client.clone();
            col = col.child(btn(format!("manage:{}:add", kind.as_str()), "Add to library").icon("plus").disabled(!state.online).render(&nav, action(move |this, _, cx| {
                let c = client.clone();
                this.act(async move { c.add_to_library(kind, id).await }, Some("Added to your library"), cx);
            }), cx));
        }
        let mut p = p.child(col);
        if let Some(path) = path {
            p = p.child(div().mt(r(8.)).text_size(r(13.)).text_color(muted()).child(path));
        }
        p
    }

    /// Cloud save status and actions of an emulator (`SaveBox`).
    pub fn save_box(&mut self, emulator_id: i64, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let emu = state.emulator(emulator_id).cloned();
        let (s, busy, latest) = state.save_sync.get(&emulator_id).cloned().unwrap_or_default();
        let installed = state.installed(ItemKind::Emulator, emulator_id).is_some();
        let paths: Vec<String> = self.client.save_paths(emulator_id).iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut rows = vec![
            ("Emulator".to_string(), emu.as_ref().map(|e| e.name.clone()).unwrap_or_default()),
            (
                "Newest on server".to_string(),
                match &latest {
                    Some(l) => format!("v{} · {}{}", l.version, fmt_relative(Some(&l.created_at)), l.device.as_ref().map(|d| format!(" · {d}")).unwrap_or_default()),
                    None => "none yet".into(),
                },
            ),
            (
                "On this device".to_string(),
                if !installed {
                    "emulator not installed".into()
                } else if let Some(v) = s.version {
                    format!("v{v}{}", if s.pending_upload == Some(true) { " + local changes (upload pending)" } else { "" })
                } else if s.pending_upload == Some(true) {
                    "upload pending".into()
                } else {
                    "not synced yet".into()
                },
            ),
        ];
        if let Some(at) = &s.synced_at {
            rows.push(("Last sync".into(), format!("{}{}", fmt_relative(Some(at)), s.last_result.as_ref().map(|r| format!(" ({r})")).unwrap_or_default())));
        }
        let mut b = div().flex().flex_col().gap(r(12.)).child(kv(rows));
        if let Some(e) = &s.last_error {
            b = b.child(div().text_size(r(13.)).text_color(err_soft()).child(e.clone()));
        }
        if !paths.is_empty() {
            let mut p = div().flex().flex_col().gap(r(4.)).text_size(r(13.)).text_color(muted()).child("Save folders");
            for x in paths {
                p = p.child(div().font_family(MONO).text_size(r(12.5)).child(x));
            }
            b = b.child(p);
        }
        let (c1, c2) = (self.client.clone(), self.client.clone());
        b.child(
            div()
                .flex()
                .flex_wrap()
                .gap(r(10.))
                .child(btn(format!("saves:{emulator_id}:sync"), if busy { "Syncing…" } else { "Sync now" }).icon("refresh").disabled(!state.online || !installed || busy).render(
                    &nav,
                    action(move |this, _, cx| {
                        let c = c1.clone();
                        this.act(async move { c.sync_now(emulator_id).await }, None, cx);
                    }),
                    cx,
                ))
                .child(btn(format!("saves:{emulator_id}:versions"), "Older versions").icon("history").disabled(!state.online).render(
                    &nav,
                    action(move |this, window, cx| {
                        this.open_modal(Modal::SaveVersions { emulator_id, versions: None }, window, cx);
                        let c = c2.clone();
                        let task = c.rt.spawn({
                            let c = c.clone();
                            async move { c.list_save_versions(emulator_id).await }
                        });
                        cx.spawn(async move |this, cx| {
                            let r = task.await;
                            let _ = this.update(cx, |this, cx| {
                                match r {
                                    Ok(Ok(v)) => {
                                        for (m, _) in this.modals.iter_mut() {
                                            if let Modal::SaveVersions { emulator_id: e, versions } = m
                                                && *e == emulator_id
                                            {
                                                *versions = Some(v.clone());
                                            }
                                        }
                                    }
                                    Ok(Err(e)) => this.toast(e.message, true, cx),
                                    Err(e) => this.toast(e.to_string(), true, cx),
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    }),
                    cx,
                )),
        )
    }

    fn emulator_panel(&mut self, id: i64, width: f32, cx: &mut Context<Self>) -> Div {
        let nav = self.nav.clone();
        let state = self.state.clone();
        let Some(e) = state.emulator(id).cloned() else { return div() };
        let installed = state.installed(ItemKind::Emulator, id).cloned();
        let mut games: Vec<_> = state.catalog.games.iter().filter(|g| g.platform == e.platform && state.library_games.contains(&g.id)).collect();
        games.sort_by(|a, b| locale_compare(&a.name, &b.name));
        let total = games.len();
        games.truncate(super::LIST_MAX);
        let meta = div().flex().gap(r(12.)).text_size(r(14.5)).text_color(text2()).child(div().font_weight(FontWeight::SEMIBOLD).child(e.platform_name.clone())).child(div().text_color(faint()).child("·")).child("Emulator");
        let hero = self.detail_hero(&AnyItem::Emu(e.clone()), None, meta);
        let actions = if !state.online && installed.is_none() {
            div().text_color(muted()).child("Not installed and you are offline.")
        } else {
            self.action_buttons(ItemKind::Emulator, id, e.size_bytes as u64, false, false, cx)
        };
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(r(32.))
            .px(r(32.))
            .py(r(20.))
            .border_b_1()
            .border_color(line())
            .child(actions)
            .child(div().flex().gap(r(36.)).ml_auto().child(stat("Size", fmt_bytes(e.size_bytes as u64))).child(stat("Games in library", total.to_string())));
        let saves = group(&nav, "panel:saves", false, || {
            let b = self.save_box(id, cx);
            panel().child(h2("Cloud saves")).child(b)
        });
        let your_games = group(&nav, "panel:games", false, || {
            let mut list = div().flex().flex_col().gap(r(2.));
            for g in games {
                let gid = g.id;
                let lid: SharedString = format!("emu-game:{gid}").into();
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
                list = list.child(focusable(row, &nav, lid, Ring::Inset(RADIUS), Some(action(move |this, window, cx| this.open_in_library(ItemKind::Game, gid, window, cx))), cx));
            }
            if total == 0 {
                list = list.child(div().text_color(muted()).child("No games for this emulator in your library."));
            } else if total > super::LIST_MAX {
                let platform = e.platform.clone();
                list = list.child(div().mt(r(10.)).child(btn("emu:all-games", format!("Show all {total} in the vault")).render(
                    &nav,
                    action(move |this, window, cx| this.show_platform_in_vault(&platform, true, window, cx)),
                    cx,
                )));
            }
            panel().child(h2("Your games")).child(list)
        });
        let manage = group(&nav, "panel:manage", false, || self.manage_panel(ItemKind::Emulator, id, &e.name, installed.as_ref().map(|i| i.path.clone()), cx));
        div().flex().flex_col().child(hero).child(bar).child(Self::panels(width, vec![saves, your_games, manage]))
    }
}
