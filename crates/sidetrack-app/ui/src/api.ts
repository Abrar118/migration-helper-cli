import type { Op, Overview, Plan, Planned, ProjectDetail, Report } from "./types";
import { demoApply, demoDetail, demoOverview, demoPlan } from "./demo";

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
 * The plan for `op` on the project at `root`, as the CLI would show it, with
 * any problem met on the way (the proxy wouldn't start) as `warnings`.
 */
export async function planOp(root: string, op: Op): Promise<Planned> {
  if (!inApp) return { plan: await demoPlan(root, op), warnings: [] };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Planned>("plan_op", { root, op });
}

/**
 * Apply the plan shown for `op` on `root`. The Rust side builds it again and
 * refuses if it changed. `onLine` gets each progress line from the applier
 * ("→ <step>" when a step starts).
 */
export async function applyOp(
  root: string,
  op: Op,
  shown: Plan,
  confirmDestructive: boolean,
  onLine: (line: string) => void,
): Promise<Report> {
  if (!inApp) return demoApply(root, shown, onLine);
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const stop = await listen<string>("apply-progress", (e) => onLine(e.payload));
  try {
    return await invoke<Report>("apply_op", { root, op, shown, confirmDestructive });
  } finally {
    stop();
  }
}

/**
 * Call `cb` with a project's root whenever the git hook reports a checkout that
 * needs a switch (a GUI client, so no terminal to ask in). Returns the unsubscribe.
 */
export function onCheckout(cb: (root: string) => void): () => void {
  if (!inApp) return () => {};
  let stop: (() => void) | null = null;
  let done = false;
  import("@tauri-apps/api/event")
    .then(({ listen }) => listen<string>("checkout", (e) => cb(e.payload)))
    .then((s) => (done ? s() : (stop = s)));
  return () => {
    done = true;
    stop?.();
  };
}
