export type Theme = "system" | "light" | "dark";

const KEY = "sidetrack.theme";

export function loadTheme(): Theme {
  try {
    const t = localStorage.getItem(KEY);
    return t === "light" || t === "dark" ? t : "system";
  } catch {
    return "system";
  }
}

const COLLAPSED = "sidetrack.sidebar-collapsed";

/** Whether the column was left collapsed. */
export function loadCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED) === "1";
  } catch {
    return false;
  }
}

export function saveCollapsed(collapsed: boolean) {
  try {
    localStorage.setItem(COLLAPSED, collapsed ? "1" : "0");
  } catch {
    // Storage blocked: it stays as set for this session.
  }
}

/** "system" follows the OS (prefers-color-scheme); the others pin a theme. */
export function applyTheme(t: Theme) {
  const root = document.documentElement;
  if (t === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", t);
  try {
    localStorage.setItem(KEY, t);
  } catch {
    // Storage blocked: the choice still applies for this session.
  }
}

/** The two looks: Press Proof (big type, proofs) and Canvas (plain type, node canvases). */
export type Look = "proof" | "canvas";

const LOOK = "sidetrack.look";

export function loadLook(): Look {
  try {
    return localStorage.getItem(LOOK) === "canvas" ? "canvas" : "proof";
  } catch {
    return "proof";
  }
}

export function applyLook(l: Look) {
  document.documentElement.setAttribute("data-look", l);
  try {
    localStorage.setItem(LOOK, l);
  } catch {
    // Storage blocked: the choice still applies for this session.
  }
}

/** A value kept per viewer (a canvas layout, the last section), never needed for correctness. */
export function loadJson<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function saveJson(key: string, value: unknown) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage blocked or full: it lasts for this session only.
  }
}
