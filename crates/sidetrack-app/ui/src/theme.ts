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
