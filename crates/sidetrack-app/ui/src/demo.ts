// Demo data for running the UI in a plain browser (`npm run dev`), where there is
// no Rust side. It is synthetic, and the app labels it "Demo data" on screen.

import type { Commit, DbBranchView, Overview, Plan, ProjectDetail, Report } from "./types";
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

// Env mode, switching to a DB branch that already exists, so the core adds a
// schema summary (`Workspace::annotate_schema`; proxy-mode switches get one too).
const shopPlan: Plan = {
  kind: "switch",
  summary: "Switch to feature_refunds_db",
  steps: [
    { type: "rewrite_env", file: ".env", variable: "DATABASE_URL", from_db: "feature_payments_db", to_db: "feature_refunds_db" },
    { type: "start_services", services: ["api"] },
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
        branches: 6, snapshots: 2, running_services: ["db", "api"],
        elsewhere: [["feature_refunds_partial_db", "~/code/shop-api-partial"]],
        migrations: { db_name: "feature_refunds_db", source: "per alembic_version", applied: 15, pending: [], unknown: [] },
        warnings: ["merged_search_db is in the registry but not on the server (removed outside sidetrack?)"],
        plan: shopPlan,
      },
      {
        root: "/home/dev/code/billing", path: "~/code/billing", name: "billing",
        mode: "proxy", listen: "127.0.0.1:54320", proxy_running: true,
        git_branch: "fix/invoice-rounding", active_db: "app_dev", problem: null,
        branches: 2, snapshots: 0, running_services: ["db", "app"], elsewhere: [],
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
        elsewhere: [["feature_sso_db", "~/code/auth-sso"]],
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

// ------------------------------------------------------------ project view

const DAY = 86400;
const now = () => Math.floor(Date.now() / 1000);

/** Commits, newest first, one every `gapHours` from `hoursAgo`. */
function commits(seed: string, subjects: string[], hoursAgo: number, gapHours = 20): Commit[] {
  return subjects.map((subject, i) => ({
    id: (seed + "0123456789abcdef").slice(i, i + 7),
    subject,
    time: now() - Math.round((hoursAgo + i * gapHours) * 3600),
  }));
}

function db(
  db_name: string,
  git_branch: string,
  parent: string | null,
  daysAgo: number,
  size: number,
  extra: Partial<DbBranchView> = {},
): DbBranchView {
  return {
    db_name, git_branch, parent,
    git_branch_exists: true, on_server: true, size_bytes: size * GiB,
    adopted: false, created_at: now() - daysAgo * DAY, last_used_at: now() - Math.min(daysAgo, 2) * DAY,
    active: false, worktree: null, snapshots: [],
    ...extra,
  };
}

function shopApi(): ProjectDetail {
  return {
    root: "/home/dev/code/shop-api", path: "~/code/shop-api", name: "shop-api", mode: "env",
    git_branch: "feature/refunds", active_db: "feature_payments_db", problem: null,
    warnings: ["merged_search_db is in the registry but not on the server (removed outside sidetrack?)"],
    services: [
      { name: "db", database: true, running: true },
      { name: "api", database: false, running: true },
      { name: "worker", database: false, running: false },
    ],
    git_branches: [
      { name: "feature/refunds", db: "feature_refunds_db", here: true, worktree: null,
        commits: commits("4f1c9e2", ["Add refunds table and model", "Refund endpoint returns 202 while it settles", "Drop payments.captured in favour of captured_at", "Wire refunds into the admin list"], 2) },
      { name: "feature/refunds-partial", db: "feature_refunds_partial_db", here: false, worktree: "~/code/shop-api-partial",
        commits: commits("b7a03d1", ["Allow partial refunds up to the captured amount", "Track refunded_amount per line item", "Partial refund e2e test"], 5) },
      { name: "fix/currency-format", db: null, here: false, worktree: null,
        commits: commits("e98d2a0", ["Format JPY without decimals"], 9) },
      { name: "feature/payments", db: "feature_payments_db", here: false, worktree: null,
        commits: commits("2c55f8b", ["Capture payments asynchronously", "Add payments.refunded_at", "Retry captures with backoff", "Payment provider sandbox config", "Payments model"], 26) },
      { name: "experiment/split-orders-into-line-items-and-fulfilment-groups", db: null, here: false, worktree: null,
        commits: commits("71d0c3a", ["Sketch line-item fulfilment groups (wip, do not merge)", "Move shipping fields onto fulfilment_groups"], 70) },
      { name: "main", db: "app_dev", here: false, worktree: null,
        commits: commits("9a8b7c6", ["Merge pull request #212 from feature/orders-split", "Bump sqlalchemy to 2.0.36", "Add index on orders.created_at", "Fix flaky checkout test", "Release 3.4.0"], 30, 30) },
    ],
    db_branches: [
      db("app_dev", "main", null, 120, 2.1, {
        adopted: true,
        snapshots: [{ db_name: "app_dev__s1", seq: 1, label: "before orders split", size_bytes: 2.0 * GiB, created_at: now() - 40 * DAY, on_server: true }],
      }),
      db("spike_cache_db", "spike/cache", "app_dev", 60, 1.9, { git_branch_exists: false }),
      db("merged_search_db", "feature/search", "app_dev", 45, 0, { git_branch_exists: false, on_server: false, size_bytes: null }),
      db("feature_payments_db", "feature/payments", "app_dev", 21, 2.2, { active: true }),
      db("feature_refunds_db", "feature/refunds", "feature_payments_db", 6, 2.2),
      db("feature_refunds_partial_db", "feature/refunds-partial", "feature_refunds_db", 3, 2.3, {
        worktree: "~/code/shop-api-partial",
        snapshots: [{ db_name: "feature_refunds_partial_db__s1", seq: 1, label: null, size_bytes: 2.3 * GiB, created_at: now() - DAY, on_server: true }],
      }),
    ],
  };
}

function billing(): ProjectDetail {
  return {
    root: "/home/dev/code/billing", path: "~/code/billing", name: "billing", mode: "proxy",
    git_branch: "fix/invoice-rounding", active_db: "app_dev", problem: null, warnings: [],
    services: [{ name: "db", database: true, running: true }, { name: "app", database: false, running: true }],
    git_branches: [
      { name: "fix/invoice-rounding", db: null, here: true, worktree: null, commits: commits("7c21aa0", ["Add rounding_mode to invoices", "Round half to even on totals"], 1) },
      { name: "feature/tax-rates", db: "feature_tax_rates_db", here: false, worktree: null, commits: commits("d4e5f60", ["Tax rates per region", "Seed default tax rates"], 30) },
      { name: "main", db: "app_dev", here: false, worktree: null, commits: commits("1a2b3c4", ["Release 1.12.0", "Invoice PDF footer"], 50) },
    ],
    db_branches: [
      db("app_dev", "main", null, 90, 1.2, { adopted: true, active: true }),
      db("feature_tax_rates_db", "feature/tax-rates", "app_dev", 12, 1.2),
    ],
  };
}

// The db container is stopped: the registry and git still show the tree, but
// whether each database exists is unknown.
function analyticsEtl(): ProjectDetail {
  return {
    root: "/home/dev/code/analytics-etl", path: "~/code/analytics-etl", name: "analytics-etl", mode: "env",
    git_branch: "main", active_db: "app_dev",
    problem: "the db service isn't running; start it with `docker compose up -d db`",
    warnings: [],
    services: [{ name: "db", database: true, running: false }, { name: "etl", database: false, running: false }],
    git_branches: [
      { name: "main", db: "app_dev", here: true, worktree: null, commits: commits("5e6f7a8", ["Nightly rollup of page views", "Pin pandas"], 48) },
      { name: "feature/backfill-2025", db: "feature_backfill_2025_db", here: false, worktree: null, commits: commits("0b1c2d3", ["Backfill 2025 events"], 100) },
    ],
    db_branches: [
      db("app_dev", "main", null, 200, 0, { adopted: true, active: true, on_server: null, size_bytes: null }),
      db("feature_backfill_2025_db", "feature/backfill-2025", "app_dev", 30, 0, { on_server: null, size_bytes: null }),
    ],
  };
}

function authService(): ProjectDetail {
  return {
    root: "/home/dev/code/auth-service", path: "~/code/auth-service", name: "auth-service", mode: "proxy",
    git_branch: "main", active_db: "app_dev", problem: null, warnings: [],
    services: [{ name: "db", database: true, running: true }, { name: "auth", database: false, running: true }],
    git_branches: [
      { name: "main", db: "app_dev", here: true, worktree: null, commits: commits("8f9e0d1", ["Rotate signing keys monthly", "Session cookie SameSite=Lax"], 4) },
      { name: "feature/sso", db: "feature_sso_db", here: false, worktree: "~/code/auth-sso", commits: commits("3c4d5e6", ["SAML metadata endpoint", "sso_connections table"], 3) },
    ],
    db_branches: [
      db("app_dev", "main", null, 75, 0.4, {
        adopted: true, active: true,
        snapshots: [{ db_name: "app_dev__s1", seq: 1, label: "clean seed", size_bytes: 0.3 * GiB, created_at: now() - 20 * DAY, on_server: true }],
      }),
      db("feature_sso_db", "feature/sso", "app_dev", 8, 0.4, { worktree: "~/code/auth-sso" }),
    ],
  };
}

const details: Record<string, () => ProjectDetail> = {
  "/home/dev/code/shop-api": shopApi,
  "/home/dev/code/billing": billing,
  "/home/dev/code/analytics-etl": analyticsEtl,
  "/home/dev/code/auth-service": authService,
};

export function demoDetail(root: string): Promise<ProjectDetail> {
  const make = details[root];
  return new Promise((resolve, reject) =>
    setTimeout(() => (make ? resolve(make()) : reject(new Error(`no demo data for ${root}`))), 200),
  );
}
