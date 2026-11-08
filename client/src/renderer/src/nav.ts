/**
 * Input layer: mouse, keyboard and game controller.
 *
 * - Arrow keys / D-pad / left stick move the focus spatially between focusable elements.
 * - Enter / A activates, Escape / Backspace / B goes back.
 * - Q,E / LB,RB (and PageUp/PageDown) switch tabs. Y / F opens filters/search (page specific), Start = menu.
 * The topmost element with [data-nav-scope] (e.g. a modal) traps navigation.
 */

export type Dir = 'up' | 'down' | 'left' | 'right';

const FOCUSABLE =
  'button:not([disabled]), a[href], input:not([disabled]):not([type="hidden"]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function scopeRoot(): HTMLElement {
  const scopes = document.querySelectorAll<HTMLElement>('[data-nav-scope]');
  return scopes.length ? scopes[scopes.length - 1] : document.body;
}

function visible(el: HTMLElement): boolean {
  if (el.closest('[aria-hidden="true"], [inert]')) return false;
  const r = el.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return false;
  const st = getComputedStyle(el);
  return st.visibility !== 'hidden' && st.display !== 'none';
}

export function focusables(root: HTMLElement = scopeRoot()): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(visible);
}

export function focusFirst(root: HTMLElement | null = scopeRoot()) {
  if (!root) return;
  const pref = root.querySelector<HTMLElement>('[data-autofocus]');
  const prefEl = pref && visible(pref) ? (pref.matches(FOCUSABLE) ? pref : focusables(pref)[0]) : undefined;
  // Prefer something that is not a text field (avoids popping up on-screen keyboards).
  const all = focusables(root);
  const el = prefEl ?? all.find((x) => !isTextInput(x)) ?? all[0];
  if (el) focusEl(el);
}

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const oy = getComputedStyle(p).overflowY;
    if ((oy === 'auto' || oy === 'scroll') && p.scrollHeight > p.clientHeight) return p;
  }
  return null;
}

export function focusEl(el: HTMLElement) {
  el.focus({ preventScroll: true });
  const behavior = inputMode === 'mouse' ? 'auto' : 'smooth';
  // Near the start / end of a scroll area: scroll all the way, so headings and hero images stay visible.
  const sp = scrollParent(el);
  if (sp) {
    const r = el.getBoundingClientRect();
    const box = sp.getBoundingClientRect();
    const fromTop = r.bottom - box.top + sp.scrollTop;
    const fromBottom = sp.scrollHeight - (r.top - box.top + sp.scrollTop);
    if (fromTop < sp.clientHeight * 0.75) return sp.scrollTo({ top: 0, behavior });
    if (fromBottom < sp.clientHeight * 0.75) return sp.scrollTo({ top: sp.scrollHeight, behavior });
  }
  el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior });
}

/*
 * Spatial navigation.
 *
 * The UI is split into regions marked with [data-nav-group] (the sidebar, a filter column, the game grid,
 * a settings panel, …). A move first looks for a target inside the current region. Only when there is none
 * does it leave the region, and then only into a region that lies completely in that direction. Without this,
 * pressing "down" at the end of a column would jump diagonally into whatever is lowest on screen.
 *
 * Inside a region, left / right only consider elements on the same row (vertical overlap), and up / down
 * prefer elements in the same column and fall back to the nearest row.
 *
 * [data-nav-remember] on a region makes entering it restore the element that was focused there last,
 * [data-nav-default] marks the element to enter a region with otherwise (e.g. the active sidebar tab).
 */
const TOL = 4;
const GROUP = '[data-nav-group]';

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

const horizontal = (d: Dir) => d === 'left' || d === 'right';

function overlap(a1: number, a2: number, b1: number, b2: number) {
  return Math.max(0, Math.min(a2, b2) - Math.max(a1, b1));
}

/** Is b (as a whole) on the `dir` side of a? */
function beyond(a: Box, b: Box, dir: Dir) {
  switch (dir) {
    case 'right':
      return b.left >= a.right - TOL;
    case 'left':
      return b.right <= a.left + TOL;
    case 'down':
      return b.top >= a.bottom - TOL;
    case 'up':
      return b.bottom <= a.top + TOL;
  }
}

/** Gap along the movement axis, overlap and gap on the cross axis. */
function distances(a: Box, b: Box, dir: Dir) {
  const primary = Math.max(
    0,
    dir === 'right' ? b.left - a.right : dir === 'left' ? a.left - b.right : dir === 'down' ? b.top - a.bottom : a.top - b.bottom,
  );
  const [a1, a2, b1, b2] = horizontal(dir) ? [a.top, a.bottom, b.top, b.bottom] : [a.left, a.right, b.left, b.right];
  const inBeam = overlap(a1, a2, b1, b2) > 0;
  const crossGap = inBeam ? 0 : Math.min(Math.abs(b1 - a2), Math.abs(a1 - b2));
  const centerDiff = Math.abs((a1 + a2) / 2 - (b1 + b2) / 2);
  return { primary, inBeam, crossGap, centerDiff };
}

/** Lower is better. Elements that line up with the current one always win over ones that don't. */
function score(a: Box, b: Box, dir: Dir, beamOnly: boolean): number | null {
  const { primary, inBeam, crossGap, centerDiff } = distances(a, b, dir);
  if (inBeam) return primary + centerDiff * (horizontal(dir) ? 0.5 : 0.1);
  if (beamOnly) return null;
  return 100000 + primary + crossGap * 2;
}

function groupOf(el: Element, root: HTMLElement): HTMLElement | null {
  const g = el.parentElement?.closest<HTMLElement>(GROUP) ?? null;
  return g && root.contains(g) && g !== root ? g : null;
}

/** The child of `container` (a group or the element itself) that holds `el`. */
function unitIn(el: HTMLElement, container: HTMLElement | null, root: HTMLElement): HTMLElement {
  let unit: HTMLElement = el;
  for (let g = groupOf(el, root); g && g !== container; g = groupOf(g, root)) unit = g;
  return unit;
}

const lastFocus = new WeakMap<HTMLElement, HTMLElement>();

function enter(unit: HTMLElement, fallback: HTMLElement, root: HTMLElement): HTMLElement {
  if (unit === fallback || !unit.matches(GROUP)) return fallback;
  if (unit.hasAttribute('data-nav-remember')) {
    const last = lastFocus.get(unit);
    if (last && last.isConnected && unit.contains(last) && root.contains(last) && visible(last)) return last;
  }
  const def = [...unit.querySelectorAll<HTMLElement>('[data-nav-default]')].find((x) => x.matches(FOCUSABLE) && visible(x));
  return def ?? fallback;
}

export function move(dir: Dir): boolean {
  const root = scopeRoot();
  const cur = document.activeElement as HTMLElement | null;
  if (!cur || cur === document.body || !root.contains(cur) || !visible(cur)) {
    if (!recoverFocus()) focusFirst(root);
    return true;
  }
  const a = cur.getBoundingClientRect();
  const list = focusables(root).filter((el) => el !== cur && !el.contains(cur) && !cur.contains(el));

  // Walk outwards: current region, then its parent region, … up to the scope root.
  let from: HTMLElement = cur;
  let container = groupOf(cur, root);
  for (;;) {
    const fromBox = from.getBoundingClientRect();
    const leaving = from !== cur;
    let best: HTMLElement | null = null;
    let bestUnit: HTMLElement | null = null;
    let bestScore = Infinity;
    for (const el of list) {
      if (container && !container.contains(el)) continue;
      if (leaving && from.contains(el)) continue;
      const unit = unitIn(el, container, root);
      const b = el.getBoundingClientRect();
      if (!beyond(a, b, dir)) continue;
      const isGroup = unit !== el;
      // Leaving a region: the target must lie completely in that direction of the region being left.
      // Entering a sub-region: the sub-region must lie in that direction of the current element.
      // Either way a region is only entered when it lines up (no diagonal jumps into the next panel).
      const ref = leaving ? fromBox : a;
      const target = isGroup ? unit.getBoundingClientRect() : b;
      if ((leaving || isGroup) && !beyond(ref, target, dir)) continue;
      if (isGroup && !distances(ref, target, dir).inBeam) continue;
      // Within a region left / right stay on the row; other regions can be entered from anywhere.
      const s = score(a, b, dir, horizontal(dir) && !isGroup && !leaving);
      if (s !== null && s < bestScore) {
        bestScore = s;
        best = el;
        bestUnit = unit;
      }
    }
    if (best && bestUnit) {
      focusEl(enter(bestUnit, best, root));
      return true;
    }
    if (!container) return false;
    from = container;
    container = groupOf(container, root);
  }
}

/**
 * The focused element disappeared (e.g. "Add to library" turned into "In library"): focus the element
 * closest to where it was, instead of starting over at the top of the page.
 */
let lastRect: DOMRect | null = null;
let lastRoot: HTMLElement | null = null;
export function recoverFocus(): boolean {
  const root = scopeRoot();
  if (!lastRect || lastRoot !== root) return false;
  const r = lastRect;
  const dist = (b: DOMRect) => Math.hypot((b.left + b.right - r.left - r.right) / 2, (b.top + b.bottom - r.top - r.bottom) / 2);
  const best = focusables(root).sort((x, y) => dist(x.getBoundingClientRect()) - dist(y.getBoundingClientRect()))[0];
  if (!best) return false;
  focusEl(best);
  return true;
}

/** Focus the default / last element of a region. */
export function focusGroup(group: HTMLElement | null): boolean {
  if (!group) return false;
  const root = scopeRoot();
  const first = focusables(group)[0];
  if (!first) return false;
  focusEl(enter(group, first, root));
  return true;
}

// ------------------------------------------------------------ input mode
export let inputMode: 'mouse' | 'keyboard' | 'gamepad' = 'mouse';
const modeListeners = new Set<(m: typeof inputMode) => void>();
export function onInputMode(cb: (m: typeof inputMode) => void) {
  modeListeners.add(cb);
  return () => {
    modeListeners.delete(cb);
  };
}
function setMode(m: typeof inputMode) {
  if (inputMode === m) return;
  inputMode = m;
  document.documentElement.dataset.input = m;
  for (const l of modeListeners) l(m);
}

// ------------------------------------------------------------ actions
export type NavAction = 'back' | 'tabPrev' | 'tabNext' | 'menu' | 'secondary' | 'tertiary';

function emit(action: NavAction) {
  window.dispatchEvent(new CustomEvent('nav', { detail: action }));
}

/** Back handlers form a stack; the most recently registered one wins. */
const backStack: (() => void)[] = [];
export function pushBack(fn: () => void) {
  backStack.push(fn);
  return () => {
    const i = backStack.lastIndexOf(fn);
    if (i >= 0) backStack.splice(i, 1);
  };
}

function activate() {
  const el = document.activeElement as HTMLElement | null;
  if (!el || el === document.body) {
    focusFirst();
    return;
  }
  if (el instanceof HTMLInputElement && (el.type === 'text' || el.type === 'password' || el.type === 'search' || el.type === 'number')) {
    el.select();
    return;
  }
  el.click();
  // If the button replaced itself, keep the focus where it was.
  if (inputMode !== 'mouse') {
    setTimeout(() => {
      const a = document.activeElement;
      if (!a || a === document.body) recoverFocus();
    }, 60);
  }
}

function back() {
  const fn = backStack[backStack.length - 1];
  if (fn) fn();
  else emit('back');
}

const isTextInput = (el: Element | null) =>
  el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && !['checkbox', 'radio', 'button', 'range'].includes(el.type));

export function installNavigation() {
  document.documentElement.dataset.input = inputMode;
  // Remember the last focused element of every [data-nav-remember] region.
  document.addEventListener('focusin', (e) => {
    const el = e.target as HTMLElement;
    lastRect = el.getBoundingClientRect();
    lastRoot = scopeRoot();
    for (let g = el.parentElement?.closest<HTMLElement>('[data-nav-remember]'); g; g = g.parentElement?.closest<HTMLElement>('[data-nav-remember]')) lastFocus.set(g, el);
  });
  window.addEventListener(
    'mousemove',
    () => setMode('mouse'),
    { passive: true },
  );
  window.addEventListener('mousedown', () => setMode('mouse'), { passive: true });

  window.addEventListener('keydown', (e) => {
    // Ctrl + / Ctrl - / Ctrl 0: UI scale (0 = automatic again).
    if ((e.ctrlKey || e.metaKey) && !e.altKey) {
      const dir = e.key === '+' || e.key === '=' ? 1 : e.key === '-' ? -1 : e.key === '0' ? 0 : null;
      if (dir !== null) {
        e.preventDefault();
        void window.nostalgia.call('stepUiScale', dir);
        return;
      }
    }
    const text = isTextInput(document.activeElement);
    const dirs: Record<string, Dir> = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };
    if (dirs[e.key]) {
      if (text && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
        // Leave a text field only when the caret is already at its edge.
        const el = document.activeElement as HTMLInputElement;
        const atStart = el.selectionStart === 0 && el.selectionEnd === 0;
        const atEnd = el.selectionStart === el.value.length;
        if ((e.key === 'ArrowLeft' && !atStart) || (e.key === 'ArrowRight' && !atEnd)) return;
      }
      if (document.activeElement instanceof HTMLSelectElement) return;
      setMode('keyboard');
      if (move(dirs[e.key])) e.preventDefault();
      return;
    }
    if (e.key === 'Escape' || (e.key === 'Backspace' && !text)) {
      setMode('keyboard');
      e.preventDefault();
      if (text) (document.activeElement as HTMLElement).blur();
      back();
      return;
    }
    if (text) return;
    if (e.key === 'Enter' || e.key === ' ') {
      setMode('keyboard');
      const el = document.activeElement;
      if (e.key === ' ' && (el instanceof HTMLButtonElement || el instanceof HTMLAnchorElement)) return; // native activation
      if (e.repeat) return;
      e.preventDefault();
      activate();
      return;
    }
    if (e.key === 'PageUp' || e.key === 'q' || e.key === 'Q') emit('tabPrev');
    else if (e.key === 'PageDown' || e.key === 'e' || e.key === 'E') emit('tabNext');
    else if (e.key === 'f' || e.key === 'F' || e.key === '/') {
      e.preventDefault();
      emit('secondary');
    } else if (e.key === 'Tab') setMode('keyboard');
  });

  startGamepadLoop();
}

// ------------------------------------------------------------ gamepad
const REPEAT_DELAY = 380;
const REPEAT_RATE = 110;
const STICK_PRESS = 0.6;
const STICK_RELEASE = 0.35;

interface PadState {
  pressed: Map<string, number>; // control -> time first pressed / last repeat
  repeated: Set<string>;
}

export const gamepadInfo = { connected: false, name: '' };

function startGamepadLoop() {
  const st: PadState = { pressed: new Map(), repeated: new Set() };
  window.addEventListener('gamepadconnected', (e) => {
    gamepadInfo.connected = true;
    gamepadInfo.name = (e as GamepadEvent).gamepad.id;
  });
  window.addEventListener('gamepaddisconnected', () => {
    gamepadInfo.connected = navigator.getGamepads().some(Boolean);
  });

  const handle = (control: string, now: number, repeatable: boolean) => {
    const first = st.pressed.get(control);
    if (first === undefined) {
      st.pressed.set(control, now);
      fire(control);
      return;
    }
    if (!repeatable) return;
    const wait = st.repeated.has(control) ? REPEAT_RATE : REPEAT_DELAY;
    if (now - first >= wait) {
      st.pressed.set(control, now);
      st.repeated.add(control);
      fire(control);
    }
  };

  const fire = (control: string) => {
    setMode('gamepad');
    switch (control) {
      case 'up':
      case 'down':
      case 'left':
      case 'right':
        move(control);
        break;
      case 'a':
        activate();
        break;
      case 'b':
        back();
        break;
      case 'lb':
        emit('tabPrev');
        break;
      case 'rb':
        emit('tabNext');
        break;
      case 'y':
        emit('secondary');
        break;
      case 'x':
        emit('tertiary');
        break;
      case 'start':
        emit('menu');
        break;
    }
  };

  /**
   * One direction per pad and frame. A diagonal on the stick (or a slightly off-axis push) used to fire
   * two directions at once, so focus went "around the corner". The stick uses the dominant axis with
   * hysteresis: it has to pass PRESS to start a direction and drop below RELEASE to end it.
   */
  const stickDir = new Map<number, Dir>();
  const padDirection = (p: Gamepad): Dir | null => {
    const b = (i: number) => !!p.buttons[i]?.pressed;
    const dpad: Dir[] = [];
    if (b(12)) dpad.push('up');
    if (b(13)) dpad.push('down');
    if (b(14)) dpad.push('left');
    if (b(15)) dpad.push('right');
    if (dpad.length) {
      const held = dpad.find((d) => st.pressed.has(d));
      return held ?? (dpad.length === 1 ? dpad[0] : null);
    }
    const ax = p.axes[0] ?? 0;
    const ay = p.axes[1] ?? 0;
    const mag = Math.hypot(ax, ay);
    const dominant: Dir = Math.abs(ax) > Math.abs(ay) ? (ax < 0 ? 'left' : 'right') : ay < 0 ? 'up' : 'down';
    const prev = stickDir.get(p.index);
    let dir: Dir | null = null;
    if (prev && mag > STICK_RELEASE && (prev === dominant || mag < STICK_PRESS)) dir = prev;
    else if (mag > STICK_PRESS) dir = dominant;
    if (dir) stickDir.set(p.index, dir);
    else stickDir.delete(p.index);
    return dir;
  };

  const loop = () => {
    requestAnimationFrame(loop);
    // Ignore input while another window (the emulator) has the focus.
    if (!document.hasFocus()) {
      st.pressed.clear();
      st.repeated.clear();
      stickDir.clear();
      return;
    }
    const pads = navigator.getGamepads().filter(Boolean) as Gamepad[];
    if (!pads.length && !st.pressed.size) return;
    const active = new Set<string>();
    const now = performance.now();
    let dir: Dir | null = null;
    for (const p of pads) {
      const b = (i: number) => !!p.buttons[i]?.pressed;
      const d = padDirection(p);
      if (d && (!dir || st.pressed.has(d))) dir = d;
      if (b(0)) active.add('a');
      if (b(1)) active.add('b');
      if (b(2)) active.add('x');
      if (b(3)) active.add('y');
      if (b(4)) active.add('lb');
      if (b(5)) active.add('rb');
      if (b(9)) active.add('start');
    }
    if (dir) active.add(dir);
    for (const c of active) handle(c, now, c === dir);
    for (const c of [...st.pressed.keys()]) {
      if (!active.has(c)) {
        st.pressed.delete(c);
        st.repeated.delete(c);
      }
    }
  };
  requestAnimationFrame(loop);
}
