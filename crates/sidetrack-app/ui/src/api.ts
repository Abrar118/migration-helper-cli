import type { Overview, Plan, ProjectDetail, Report } from "./types";
import { demoApply, demoDetail, demoOverview } from "./demo";

/** Inside the Tauri window; otherwise (plain `vite` in a browser) demo data is used. */
export const inApp = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function loadOverview(): Promise<Overview> {
  if (!inApp) return demoOverview();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Overview>("overview");
}

/** One project's git branches, DB branch tree, containers and recent commits. */
export async function loadDetail(root: string): Promise<ProjectDetail> {
  if (!inApp) return demoDetail(root);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProjectDetail>("project_detail", { root });
}

/**
 * Apply the switch plan shown for `root`. `onLine` gets each progress line from
 * the applier ("→ <step>" when a step starts).
 */
export async function applySwitch(
  root: string,
  shown: Plan,
  confirmDestructive: boolean,
  onLine: (line: string) => void,
): Promise<Report> {
  if (!inApp) return demoApply(shown, onLine);
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const stop = await listen<string>("apply-progress", (e) => onLine(e.payload));
  try {
    return await invoke<Report>("apply_switch", { root, shown, confirmDestructive });
  } finally {
    stop();
  }
}
