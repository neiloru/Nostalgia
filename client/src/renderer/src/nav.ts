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

export function focusEl(el: HTMLElement) {
  el.focus({ preventScroll: true });
  el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: inputMode === 'mouse' ? 'auto' : 'smooth' });
}

function overlap(a1: number, a2: number, b1: number, b2: number) {
  return Math.max(0, Math.min(a2, b2) - Math.max(a1, b1));
}

export function move(dir: Dir): boolean {
  const root = scopeRoot();
  const cur = document.activeElement as HTMLElement | null;
  const list = focusables(root);
  if (!cur || cur === document.body || !root.contains(cur) || !visible(cur)) {
    focusFirst(root);
    return true;
  }
  const a = cur.getBoundingClientRect();
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  for (const el of list) {
    if (el === cur || el.contains(cur) || cur.contains(el)) continue;
    const b = el.getBoundingClientRect();
    let primary: number;
    let secondary: number;
    const tol = 4;
    switch (dir) {
      case 'right':
        if (b.left < a.right - tol && !(b.left > a.left + a.width / 2)) continue;
        primary = Math.max(0, b.left - a.right);
        secondary = overlap(a.top, a.bottom, b.top, b.bottom) > 0 ? 0 : Math.min(Math.abs(b.top - a.bottom), Math.abs(a.top - b.bottom));
        break;
      case 'left':
        if (b.right > a.left + tol && !(b.right < a.right - a.width / 2)) continue;
        primary = Math.max(0, a.left - b.right);
        secondary = overlap(a.top, a.bottom, b.top, b.bottom) > 0 ? 0 : Math.min(Math.abs(b.top - a.bottom), Math.abs(a.top - b.bottom));
        break;
      case 'down':
        if (b.top < a.bottom - tol && !(b.top > a.top + a.height / 2)) continue;
        primary = Math.max(0, b.top - a.bottom);
        secondary = overlap(a.left, a.right, b.left, b.right) > 0 ? Math.abs(b.left - a.left) * 0.05 : Math.min(Math.abs(b.left - a.right), Math.abs(a.left - b.right));
        break;
      case 'up':
        if (b.bottom > a.top + tol && !(b.bottom < a.bottom - a.height / 2)) continue;
        primary = Math.max(0, a.top - b.bottom);
        secondary = overlap(a.left, a.right, b.left, b.right) > 0 ? Math.abs(b.left - a.left) * 0.05 : Math.min(Math.abs(b.left - a.right), Math.abs(a.left - b.right));
        break;
    }
    const score = primary + secondary * 2.5;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }
  if (best) {
    focusEl(best);
    return true;
  }
  return false;
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
  window.addEventListener(
    'mousemove',
    () => setMode('mouse'),
    { passive: true },
  );
  window.addEventListener('mousedown', () => setMode('mouse'), { passive: true });

  window.addEventListener('keydown', (e) => {
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
const DEADZONE = 0.5;

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

  const loop = () => {
    requestAnimationFrame(loop);
    // Ignore input while another window (the emulator) has the focus.
    if (!document.hasFocus()) {
      st.pressed.clear();
      st.repeated.clear();
      return;
    }
    const pads = navigator.getGamepads().filter(Boolean) as Gamepad[];
    if (!pads.length && !st.pressed.size) return;
    const active = new Set<string>();
    const now = performance.now();
    for (const p of pads) {
      const b = (i: number) => !!p.buttons[i]?.pressed;
      const ax = p.axes[0] ?? 0;
      const ay = p.axes[1] ?? 0;
      if (b(12) || ay < -DEADZONE) active.add('up');
      if (b(13) || ay > DEADZONE) active.add('down');
      if (b(14) || ax < -DEADZONE) active.add('left');
      if (b(15) || ax > DEADZONE) active.add('right');
      if (b(0)) active.add('a');
      if (b(1)) active.add('b');
      if (b(2)) active.add('x');
      if (b(3)) active.add('y');
      if (b(4)) active.add('lb');
      if (b(5)) active.add('rb');
      if (b(9)) active.add('start');
    }
    for (const c of active) handle(c, now, ['up', 'down', 'left', 'right'].includes(c));
    for (const c of [...st.pressed.keys()]) {
      if (!active.has(c)) {
        st.pressed.delete(c);
        st.repeated.delete(c);
      }
    }
  };
  requestAnimationFrame(loop);
}
