import type { ReactNode } from "react";
import type { Plan, Step } from "./types";
import { bytes } from "./format";

/** The ink a step is printed in on the proof's colour bar. */
export type Ink = "key" | "cyan" | "magenta" | "yellow" | "spot";

const BOOKKEEPING = new Set([
  "register_branch",
  "record_migrations",
  "touch_branch",
  "register_snapshot",
  "forget_branch",
  "forget_snapshot",
  "relink_branch",
]);

export const isBookkeeping = (s: Step) => BOOKKEEPING.has(s.type);
export const isDestructive = (s: Step) => s.type === "drop_database";

/** Steps shown on the proof, with their index in `plan.steps`. */
export function visibleSteps(plan: Plan): { step: Step; index: number }[] {
  return plan.steps
    .map((step, index) => ({ step, index }))
    .filter(({ step }) => !isBookkeeping(step));
}

export function ink(s: Step): Ink {
  switch (s.type) {
    case "clone_database":
    case "create_empty_database":
    case "freeze_database":
    case "rename_database":
      return "cyan";
    case "rewrite_env":
    case "set_proxy_target":
    case "set_env_port":
      return "magenta";
    case "run_migrations":
    case "run_seed":
      return "yellow";
    case "drop_database":
    case "drop_temporary_database":
      return "spot";
    default:
      return "key";
  }
}

/** One or two words for the colour-bar patch. */
export function label(s: Step): string {
  switch (s.type) {
    case "stop_services": return "Stop";
    case "start_services": return "Restart";
    case "terminate_connections": return "Disconnect";
    case "hold_connections": return "Hold";
    case "release_connections": return "Release";
    case "clone_database": return "Copy";
    case "create_empty_database": return "Create";
    case "freeze_database": return "Freeze";
    case "rename_database": return "Rename";
    case "drop_database": return "Drop";
    case "drop_temporary_database": return "Clean up";
    case "rewrite_env": return "Repoint";
    case "set_proxy_target": return "Route";
    case "set_env_port": return "Port";
    case "run_migrations": return "Migrate";
    case "run_seed": return "Seed";
    default: return "Record";
  }
}

const Db = ({ children }: { children: ReactNode }) => <span className="db">{children}</span>;
const list = (xs: string[]) => xs.join(", ");
const file = (p: string) => p.split("/").pop() ?? p;

/** The step as a sentence, database names set in the database face. */
export function describe(s: Step): ReactNode {
  switch (s.type) {
    case "stop_services":
      return <>Stop {list(s.services)} while the database is copied</>;
    case "start_services":
      return <>Recreate {list(s.services)} so {s.services.length === 1 ? "it picks" : "they pick"} up the change</>;
    case "terminate_connections":
      return <>Close {s.sessions.length} open connection{s.sessions.length === 1 ? "" : "s"} to <Db>{s.db}</Db></>;
    case "hold_connections":
      return <>Hold new connections to <Db>{s.db}</Db> while it's copied</>;
    case "release_connections":
      return <>Release connections to <Db>{s.db}</Db></>;
    case "clone_database":
      return <>Create <Db>{s.target}</Db> as a copy of <Db>{s.source}</Db></>;
    case "create_empty_database":
      return <>Create the empty database <Db>{s.name}</Db></>;
    case "freeze_database":
      return <>Freeze <Db>{s.name}</Db> so nothing can connect to it</>;
    case "drop_database":
      return <>Drop <Db>{s.name}</Db> and everything in it</>;
    case "drop_temporary_database":
      return <>Drop the temporary copy <Db>{s.name}</Db></>;
    case "rename_database":
      return <>Rename <Db>{s.from}</Db> to <Db>{s.to}</Db></>;
    case "rewrite_env":
      return <>Point {s.variable} in {s.file} at <Db>{s.to_db}</Db> (was <Db>{s.from_db}</Db>)</>;
    case "set_proxy_target":
      return <>Route <Db>{s.database}</Db> through the proxy to <Db>{s.to_db}</Db></>;
    case "set_env_port":
      return <>{s.copy_from ? <>Copy {s.file} from the main worktree and point</> : <>Point</>} {s.variable} at this worktree's proxy port {s.port}</>;
    case "run_migrations":
      return (
        <>
          Run {s.pending.length} migration{s.pending.length === 1 ? "" : "s"}
          {s.pending.length > 0 && <>: {s.pending.slice(0, 3).map(file).join(", ")}{s.pending.length > 3 && `, and ${s.pending.length - 3} more`}</>}
        </>
      );
    case "run_seed":
      return <>Seed the new database</>;
    default:
      return <>Update sidetrack's records</>;
  }
}

/** Extra detail on the right of a step line. */
export function meta(s: Step): string | null {
  return s.type === "clone_database" ? bytes(s.size_bytes) : null;
}

/** The shell command a step runs, if any (shown on hover). */
export function command(s: Step): string | undefined {
  return s.type === "run_migrations" || s.type === "run_seed" ? s.command : undefined;
}

/** The switch target in a plan: the database the app will use afterwards. */
export function target(plan: Plan): string | null {
  for (const s of plan.steps) {
    if (s.type === "set_proxy_target" || s.type === "rewrite_env") return s.to_db;
  }
  return null;
}

/** True when the plan creates the target database (the branch had none). */
export function createsDatabase(plan: Plan): boolean {
  return plan.steps.some((s) => s.type === "clone_database" || s.type === "create_empty_database");
}

export function addedBytes(plan: Plan): number {
  return plan.steps.reduce((n, s) => n + (s.type === "clone_database" ? s.size_bytes : 0), 0);
}
