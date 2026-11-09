//! Controller / keyboard navigation (port of the Electron renderer's nav.ts).
//!
//! Every focusable element registers itself while the UI is rendered and records its painted
//! bounds. Moves are spatial:
//!  - the UI is split into regions (groups): the tab bar, a filter column, the game grid, a panel …
//!  - a move first looks inside the current region and only leaves it into a region that lies
//!    completely in that direction *and* lines up with it (no diagonal jumps);
//!  - inside a region left / right stay on the same row, up / down prefer the same column;
//!  - regions marked `remember` restore their last focused element, `default` items are entered first.
//! Modals open a new scope that traps navigation.

use super::AppView;
use gpui_kit::component::input::InputState;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type ActionFn = Rc<dyn Fn(&mut AppView, &mut Window, &mut Context<AppView>)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    Mouse,
    Keyboard,
    Gamepad,
}

pub struct NavItem {
    pub id: SharedString,
    pub groups: Vec<usize>,
    pub scope: u32,
    pub scroll: Option<SharedString>,
    pub action: Option<ActionFn>,
    pub input: Option<Entity<InputState>>,
    pub default: bool,
    pub autofocus: bool,
}

struct NavGroup {
    key: SharedString,
    remember: bool,
}

pub struct Nav {
    items: Vec<NavItem>,
    groups: Vec<NavGroup>,
    group_stack: Vec<usize>,
    scope_stack: Vec<u32>,
    scroll_stack: Vec<SharedString>,
    scrolls: HashMap<SharedString, ScrollHandle>,
    pub focused: Option<SharedString>,
    pub mode: InputMode,
    last_focus: HashMap<SharedString, SharedString>,
    last_rect: Option<Bounds<Pixels>>,
    last_scope: u32,
    /// Painted bounds of items ("i:<id>") and groups ("g:<key>"), written during prepaint.
    pub bounds: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
}

pub type NavRc = Rc<RefCell<Nav>>;

const TOL: f32 = 4.0;

fn beyond(a: &Bounds<Pixels>, b: &Bounds<Pixels>, dir: Dir) -> bool {
    let t = px(TOL);
    match dir {
        Dir::Right => b.left() >= a.right() - t,
        Dir::Left => b.right() <= a.left() + t,
        Dir::Down => b.top() >= a.bottom() - t,
        Dir::Up => b.bottom() <= a.top() + t,
    }
}

fn overlap(a1: f32, a2: f32, b1: f32, b2: f32) -> f32 {
    (a2.min(b2) - a1.max(b1)).max(0.0)
}

struct Dist {
    primary: f32,
    in_beam: bool,
    cross_gap: f32,
    center_diff: f32,
}

fn f(p: Pixels) -> f32 {
    f32::from(p)
}

fn distances(a: &Bounds<Pixels>, b: &Bounds<Pixels>, dir: Dir) -> Dist {
    let primary = match dir {
        Dir::Right => f(b.left()) - f(a.right()),
        Dir::Left => f(a.left()) - f(b.right()),
        Dir::Down => f(b.top()) - f(a.bottom()),
        Dir::Up => f(a.top()) - f(b.bottom()),
    }
    .max(0.0);
    let (a1, a2, b1, b2) = if dir.horizontal() {
        (f(a.top()), f(a.bottom()), f(b.top()), f(b.bottom()))
    } else {
        (f(a.left()), f(a.right()), f(b.left()), f(b.right()))
    };
    let in_beam = overlap(a1, a2, b1, b2) > 0.0;
    let cross_gap = if in_beam { 0.0 } else { (b1 - a2).abs().min((a1 - b2).abs()) };
    let center_diff = ((a1 + a2) / 2.0 - (b1 + b2) / 2.0).abs();
    Dist { primary, in_beam, cross_gap, center_diff }
}

/// Lower is better. Elements that line up with the current one always win over ones that don't.
fn score(a: &Bounds<Pixels>, b: &Bounds<Pixels>, dir: Dir, beam_only: bool) -> Option<f32> {
    let d = distances(a, b, dir);
    if d.in_beam {
        return Some(d.primary + d.center_diff * if dir.horizontal() { 0.5 } else { 0.1 });
    }
    if beam_only {
        return None;
    }
    Some(100_000.0 + d.primary + d.cross_gap * 2.0)
}

impl Nav {
    pub fn new() -> NavRc {
        Rc::new(RefCell::new(Nav {
            items: Vec::new(),
            groups: Vec::new(),
            group_stack: Vec::new(),
            scope_stack: vec![0],
            scroll_stack: Vec::new(),
            scrolls: HashMap::new(),
            focused: None,
            mode: InputMode::Mouse,
            last_focus: HashMap::new(),
            last_rect: None,
            last_scope: 0,
            bounds: Rc::new(RefCell::new(HashMap::new())),
        }))
    }

    /// Called at the start of every render.
    pub fn begin_frame(&mut self) {
        self.items.clear();
        self.groups.clear();
        self.group_stack.clear();
        self.scope_stack = vec![0];
        self.scroll_stack.clear();
        self.scrolls.clear();
        self.bounds.borrow_mut().clear();
    }

    pub fn push_group(&mut self, key: SharedString, remember: bool) {
        self.groups.push(NavGroup { key, remember });
        self.group_stack.push(self.groups.len() - 1);
    }
    pub fn pop_group(&mut self) {
        self.group_stack.pop();
    }
    pub fn push_scope(&mut self) {
        let next = self.scope_stack.iter().max().copied().unwrap_or(0).max(self.top_scope()) + 1;
        self.scope_stack.push(next);
    }
    pub fn pop_scope(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }
    pub fn push_scroll(&mut self, key: SharedString, handle: ScrollHandle) {
        self.scrolls.insert(key.clone(), handle);
        self.scroll_stack.push(key);
    }
    pub fn pop_scroll(&mut self) {
        self.scroll_stack.pop();
    }

    pub fn add(&mut self, id: SharedString, action: Option<ActionFn>, input: Option<Entity<InputState>>, default: bool, autofocus: bool) {
        self.items.push(NavItem {
            id,
            groups: self.group_stack.clone(),
            scope: *self.scope_stack.last().unwrap_or(&0),
            scroll: self.scroll_stack.last().cloned(),
            action,
            input,
            default,
            autofocus,
        });
    }

    /// Make an already registered item the page's initial focus (`data-autofocus`).
    pub fn set_autofocus(&mut self, id: &str) {
        if let Some(it) = self.items.iter_mut().rev().find(|i| i.id.as_ref() == id) {
            it.autofocus = true;
        }
    }

    pub fn is_focused(&self, id: &str) -> bool {
        self.focused.as_deref() == Some(id)
    }

    /// The focus ring is drawn for keyboard and controller only.
    pub fn ring(&self, id: &str) -> bool {
        self.mode != InputMode::Mouse && self.is_focused(id)
    }

    fn top_scope(&self) -> u32 {
        self.items.iter().map(|i| i.scope).max().unwrap_or(0)
    }

    fn item_bounds(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.bounds.borrow().get(&SharedString::from(format!("i:{id}"))).copied()
    }

    fn group_bounds(&self, g: usize) -> Option<Bounds<Pixels>> {
        self.bounds.borrow().get(&SharedString::from(format!("g:{}", self.groups[g].key))).copied()
    }

    fn visible(&self, scope: u32) -> Vec<(usize, Bounds<Pixels>)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.scope == scope)
            .filter_map(|(i, it)| self.item_bounds(&it.id).filter(|b| f(b.size.width) > 0.0 && f(b.size.height) > 0.0).map(|b| (i, b)))
            .collect()
    }

    pub fn item(&self, id: &str) -> Option<&NavItem> {
        self.items.iter().find(|i| i.id.as_ref() == id)
    }

    /// Index of an item in the top scope.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        let scope = self.top_scope();
        self.items.iter().position(|i| i.id.as_ref() == id && i.scope == scope)
    }

    pub fn focused_item(&self) -> Option<&NavItem> {
        self.focused.as_deref().and_then(|f| self.item(f))
    }

    /// The child of `container` (a group index, or the item itself) that holds item `i`.
    fn unit_in(&self, i: usize, container: Option<usize>) -> Option<usize> {
        let groups = &self.items[i].groups;
        match container {
            None => groups.first().copied(),
            Some(c) => groups.iter().position(|g| *g == c).and_then(|p| groups.get(p + 1).copied()),
        }
    }

    fn enter(&self, unit: Option<usize>, fallback: usize, scope: u32) -> usize {
        let Some(g) = unit else { return fallback };
        if self.groups[g].remember
            && let Some(last) = self.last_focus.get(&self.groups[g].key)
            && let Some(idx) = self.items.iter().position(|i| &i.id == last && i.scope == scope && i.groups.contains(&g))
            && self.item_bounds(&self.items[idx].id).is_some()
        {
            return idx;
        }
        self.items
            .iter()
            .position(|i| i.default && i.scope == scope && i.groups.contains(&g) && self.item_bounds(&i.id).is_some())
            .unwrap_or(fallback)
    }

    /// Spatial move. Returns the item to focus.
    pub fn find_move(&self, dir: Dir) -> Option<usize> {
        let scope = self.top_scope();
        let list = self.visible(scope);
        let cur = self.focused.as_deref().and_then(|f| self.items.iter().position(|i| i.id.as_ref() == f && i.scope == scope));
        let Some(cur) = cur else { return self.first_index(None) };
        let a = self.item_bounds(&self.items[cur].id)?;
        let mut container = self.items[cur].groups.last().copied();
        let mut from: Option<usize> = None; // None = the current item, Some(g) = the region being left
        loop {
            let leaving = from.is_some();
            let from_box = match from {
                Some(g) => self.group_bounds(g).unwrap_or(a),
                None => a,
            };
            let mut best: Option<(usize, Option<usize>, f32)> = None;
            for (i, b) in &list {
                let (i, b) = (*i, *b);
                if i == cur {
                    continue;
                }
                let groups = &self.items[i].groups;
                if let Some(c) = container
                    && !groups.contains(&c)
                {
                    continue;
                }
                if let Some(g) = from
                    && groups.contains(&g)
                {
                    continue;
                }
                let unit = self.unit_in(i, container);
                if !beyond(&a, &b, dir) {
                    continue;
                }
                let is_group = unit.is_some();
                let reference = if leaving { from_box } else { a };
                let target = match unit {
                    Some(g) => self.group_bounds(g).unwrap_or(b),
                    None => b,
                };
                // Leaving a region: the target must lie completely in that direction of the region being left.
                // Entering a sub-region: the sub-region must lie in that direction of the current element.
                // Either way a region is only entered when it lines up (no diagonal jumps into the next panel).
                if (leaving || is_group) && !beyond(&reference, &target, dir) {
                    continue;
                }
                if is_group && !distances(&reference, &target, dir).in_beam {
                    continue;
                }
                // Within a region left / right stay on the row; other regions can be entered from anywhere.
                if let Some(s) = score(&a, &b, dir, dir.horizontal() && !is_group && !leaving)
                    && best.is_none_or(|(_, _, bs)| s < bs)
                {
                    best = Some((i, unit, s));
                }
            }
            if let Some((i, unit, _)) = best {
                return Some(self.enter(unit, i, scope));
            }
            let c = container?;
            from = Some(c);
            // Parent of `c`: the group before it in any item's path.
            container = self.items.iter().find_map(|it| it.groups.iter().position(|g| *g == c).map(|p| if p == 0 { None } else { Some(it.groups[p - 1]) })).flatten();
        }
    }

    /// First item of the top scope (or of a group): `autofocus` first, then anything that is not a text field.
    pub fn first_index(&self, group: Option<&str>) -> Option<usize> {
        let scope = self.top_scope();
        let in_group = |i: &NavItem| match group {
            None => true,
            Some(k) => i.groups.iter().any(|g| self.groups[*g].key.as_ref() == k),
        };
        let cands: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.scope == scope && in_group(it) && self.item_bounds(&it.id).is_some())
            .map(|(i, _)| i)
            .collect();
        cands
            .iter()
            .copied()
            .find(|i| self.items[*i].autofocus)
            .or_else(|| cands.iter().copied().find(|i| self.items[*i].input.is_none()))
            .or_else(|| cands.first().copied())
    }

    /// Default / remembered item of a group.
    pub fn group_entry(&self, key: &str) -> Option<usize> {
        let scope = self.top_scope();
        let g = self.groups.iter().position(|g| g.key.as_ref() == key)?;
        let first = self.items.iter().position(|i| i.scope == scope && i.groups.contains(&g) && self.item_bounds(&i.id).is_some())?;
        Some(self.enter(Some(g), first, scope))
    }

    /// The focused element disappeared: the element closest to where it was.
    pub fn recover_index(&self) -> Option<usize> {
        let r = self.last_rect?;
        let scope = self.top_scope();
        if scope != self.last_scope {
            return None;
        }
        let center = |b: &Bounds<Pixels>| (f(b.left()) + f(b.right())) / 2.0;
        let middle = |b: &Bounds<Pixels>| (f(b.top()) + f(b.bottom())) / 2.0;
        self.visible(scope)
            .into_iter()
            .min_by(|(_, x), (_, y)| {
                let dx = |b: &Bounds<Pixels>| ((center(b) - center(&r)).powi(2) + (middle(b) - middle(&r)).powi(2)).sqrt();
                dx(x).total_cmp(&dx(y))
            })
            .map(|(i, _)| i)
    }

    pub fn focused_present(&self) -> bool {
        let scope = self.top_scope();
        self.focused.as_deref().is_some_and(|f| self.items.iter().any(|i| i.id.as_ref() == f && i.scope == scope && self.item_bounds(f).is_some()))
    }

    /// Mark item `i` as focused (remembering it for its regions) and scroll it into view.
    pub fn set_focus_index(&mut self, i: usize) -> SharedString {
        let id = self.items[i].id.clone();
        for g in &self.items[i].groups {
            if self.groups[*g].remember {
                self.last_focus.insert(self.groups[*g].key.clone(), id.clone());
            }
        }
        self.focused = Some(id.clone());
        self.last_scope = self.items[i].scope;
        if let Some(b) = self.item_bounds(&id) {
            self.last_rect = Some(b);
            if let Some(key) = &self.items[i].scroll
                && let Some(handle) = self.scrolls.get(key)
            {
                scroll_into_view(handle, b);
            }
        }
        id
    }

    /// Focus by id (e.g. after a mouse click); remembers regions, no scrolling.
    pub fn set_focus_id(&mut self, id: &str) {
        if let Some(i) = self.items.iter().position(|it| it.id.as_ref() == id) {
            for g in self.items[i].groups.clone() {
                if self.groups[g].remember {
                    self.last_focus.insert(self.groups[g].key.clone(), self.items[i].id.clone());
                }
            }
            self.last_scope = self.items[i].scope;
            self.last_rect = self.item_bounds(id);
        }
        self.focused = Some(id.into());
    }
}

/// Near the start / end of a scroll area: scroll all the way, so headings and hero images stay
/// visible. Otherwise just far enough to show the element with a small margin.
fn scroll_into_view(handle: &ScrollHandle, b: Bounds<Pixels>) {
    let area = handle.bounds();
    let viewport = f(area.size.height);
    if viewport <= 0.0 {
        return;
    }
    let max = f(handle.max_offset().y).max(0.0);
    let offset = handle.offset();
    let scroll_top = -f(offset.y);
    let el_top = f(b.top()) - f(area.top()) + scroll_top;
    let el_bottom = f(b.bottom()) - f(area.top()) + scroll_top;
    let margin = 24.0;
    let target = if el_bottom < viewport * 0.75 {
        0.0
    } else if (max + viewport) - el_top < viewport * 0.75 {
        max
    } else if el_top - margin < scroll_top {
        el_top - margin
    } else if el_bottom + margin > scroll_top + viewport {
        el_bottom + margin - viewport
    } else {
        scroll_top
    };
    let target = target.clamp(0.0, max);
    if (target - scroll_top).abs() > 0.5 {
        handle.set_offset(point(offset.x, px(-target)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    fn nav_with(items: &[(&str, &[&str], Bounds<Pixels>)]) -> Nav {
        let rc = Nav::new();
        let mut n = Rc::try_unwrap(rc).ok().unwrap().into_inner();
        for (id, groups, bounds) in items {
            for g in *groups {
                if !n.groups.iter().any(|x| x.key.as_ref() == *g) {
                    n.groups.push(NavGroup { key: g.to_string().into(), remember: true });
                }
            }
            let gidx: Vec<usize> = groups.iter().map(|g| n.groups.iter().position(|x| x.key.as_ref() == *g).unwrap()).collect();
            n.items.push(NavItem { id: id.to_string().into(), groups: gidx, scope: 0, scroll: None, action: None, input: None, default: false, autofocus: false });
            n.bounds.borrow_mut().insert(format!("i:{id}").into(), *bounds);
        }
        n
    }

    #[core::prelude::v1::test]
    fn stays_in_row_and_does_not_jump_diagonally() {
        // Two cards in a grid region, a filter column to the left, the tab bar above.
        let mut n = nav_with(&[
            ("tab", &["bar"], b(20., 0., 80., 40.)),
            ("filter-top", &["content", "filters"], b(0., 60., 200., 40.)),
            ("filter-bottom", &["content", "filters"], b(0., 600., 200., 40.)),
            ("card1", &["content", "grid"], b(300., 60., 150., 200.)),
            ("card2", &["content", "grid"], b(470., 60., 150., 200.)),
        ]);
        for (k, bb) in [("g:bar", b(0., 0., 1000., 50.)), ("g:content", b(0., 50., 1000., 700.)), ("g:filters", b(0., 50., 250., 700.)), ("g:grid", b(250., 50., 750., 700.))] {
            n.bounds.borrow_mut().insert(k.into(), bb);
        }
        n.focused = Some("card2".into());
        assert_eq!(n.find_move(Dir::Right), None, "end of the row stays");
        assert_eq!(n.items[n.find_move(Dir::Left).unwrap()].id.as_ref(), "card1");
        n.focused = Some("filter-bottom".into());
        assert_eq!(n.find_move(Dir::Down), None, "no jump into the grid");
        assert_eq!(n.items[n.find_move(Dir::Right).unwrap()].id.as_ref(), "card1");
        n.focused = Some("card1".into());
        assert_eq!(n.items[n.find_move(Dir::Up).unwrap()].id.as_ref(), "tab");
    }
}
