// Demo data for running the UI in a plain browser (`npm run dev`), where there is
// no Rust side. It is synthetic, and the app labels it "Demo data" on screen.

import type { Overview, Plan, Report } from "./types";
import { visibleSteps } from "./steps";

const GiB = 1024 ** 3;

const billingPlan: Plan = {
  kind: "switch",
  summary: "Create fix_invoice_rounding_db from app_dev and switch to it",
  steps: [
    { type: "hold_connections", db: "app_dev" },
    { type: "clone_database", source: "app_dev", target: "fix_invoice_rounding_db", size_bytes: 1.2 * GiB },
    { type: "release_connections", db: "app_dev" },
    { type: "set_proxy_target", database: "app_dev", from_db: "app_dev", to_db: "fix_invoice_rounding_db" },
    {
      type: "run_migrations",
      command: "docker compose exec -T api alembic upgrade head",
      pending: ["migrations/versions/7c21_add_rounding_mode.py"],
    },
    { type: "register_branch", git_branch: "fix/invoice-rounding", db_name: "fix_invoice_rounding_db", parent_db: "app_dev" },
    { type: "record_migrations", db_name: "fix_invoice_rounding_db" },
  ],
  warnings: [],
  notes: ["app_dev has every migration fix/invoice-rounding shares with main, so it's the closest base"],
};

// Env mode, switching to a DB branch that already exists: the only case where the
// core adds a schema summary (`Workspace::annotate_schema`).
const shopPlan: Plan = {
  kind: "switch",
  summary: "Switch to feature_refunds_db",
  steps: [
    { type: "rewrite_env", file: ".env", variable: "DATABASE_URL", from_db: "feature_payments_db", to_db: "feature_refunds_db" },
    { type: "start_services", services: ["api", "worker"] },
    { type: "touch_branch", db_name: "feature_refunds_db" },
  ],
  warnings: [],
  notes: [],
  schema: {
    from_db: "feature_payments_db",
    to_db: "feature_refunds_db",
    added_tables: ["public.refunds"],
    removed_tables: [],
    changed_tables: [{ table: "public.payments", added_columns: ["refunded_at timestamptz"], removed_columns: ["captured boolean"] }],
  },
};

export function demoOverview(): Promise<Overview> {
  const overview: Overview = {
    registry: "~/.local/share/sidetrack/registry.sqlite",
    disk: { free_bytes: 41.3 * GiB, total_bytes: 58 * GiB },
    projects: [
      {
        root: "/home/dev/code/shop-api", path: "~/code/shop-api", name: "shop-api",
        mode: "env", listen: null, proxy_running: null,
        git_branch: "feature/refunds", active_db: "feature_payments_db", problem: null,
        branches: 6, snapshots: 2, running_services: ["db", "api", "worker"], elsewhere: [],
        migrations: { db_name: "feature_refunds_db", source: "per alembic_version", applied: 15, pending: [], unknown: [] },
        warnings: ["merged_search_db is in the registry but not on the server (removed outside sidetrack?)"],
        plan: shopPlan,
      },
      {
        root: "/home/dev/code/billing", path: "~/code/billing", name: "billing",
        mode: "proxy", listen: "127.0.0.1:54320", proxy_running: true,
        git_branch: "fix/invoice-rounding", active_db: "app_dev", problem: null,
        branches: 3, snapshots: 0, running_services: ["db", "app"], elsewhere: [],
        migrations: null, warnings: [], plan: billingPlan,
      },
      {
        root: "/home/dev/code/analytics-etl", path: "~/code/analytics-etl", name: "analytics-etl",
        mode: "env", listen: null, proxy_running: null,
        git_branch: "main", active_db: "app_dev",
        problem: "the db service isn't running; start it with `docker compose up -d db`",
        branches: 0, snapshots: 0, running_services: [], elsewhere: [], migrations: null, warnings: [], plan: null,
      },
      {
        root: "/home/dev/code/auth-service", path: "~/code/auth-service", name: "auth-service",
        mode: "proxy", listen: "127.0.0.1:54320", proxy_running: true,
        git_branch: "main", active_db: "app_dev", problem: null,
        branches: 2, snapshots: 1, running_services: ["db", "auth"],
        elsewhere: [["sso_db", "~/code/auth-sso"]],
        migrations: { db_name: "app_dev", source: "per sidetrack's records", applied: 9, pending: [], unknown: [] },
        warnings: [], plan: null,
      },
    ],
  };
  return new Promise((r) => setTimeout(() => r(overview), 250));
}

/** Pretend to apply: one step every ~0.9 s, then done. */
export async function demoApply(plan: Plan, onLine: (line: string) => void): Promise<Report> {
  const timings: Report["timings"] = [];
  for (const { index } of visibleSteps(plan)) {
    onLine("→ step");
    const seconds = 0.6 + Math.random() * 0.8;
    await new Promise((r) => setTimeout(r, seconds * 1000));
    timings.push({ step: index, seconds, detail: null });
  }
  return { outcome: { status: "done" }, log: [], timings };
}
