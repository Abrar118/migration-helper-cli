// Demo data for running the UI in a plain browser (`npm run dev`), where there is
// no Rust side. It is synthetic, and the app labels it "Demo data" on screen.

import type { AddPreview, Commit, DbBranchView, Op, Overview, Plan, ProjectDetail, Report, ServiceView, Step, StructureView } from "./types";
import { visibleSteps } from "./steps";
import { bytes } from "./format";

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

/**
 * Pretend to apply: one step every ~0.9 s, then done. Deleting billing's
 * feature_tax_rates_db fails on its first step, to show a plan that didn't apply.
 */
export async function demoApply(root: string, plan: Plan, onLine: (line: string) => void): Promise<Report> {
  const timings: Report["timings"] = [];
  for (const { index } of visibleSteps(plan)) {
    onLine("→ step");
    if (root === "/home/dev/code/billing" && plan.kind === "delete") {
      await new Promise((r) => setTimeout(r, 1200));
      return {
        outcome: {
          status: "rolled_back",
          step: "Drop feature_tax_rates_db",
          error: 'database "feature_tax_rates_db" is being accessed by other users (a psql shell is still connected)',
          undo_errors: [],
        },
        log: [],
        timings,
      };
    }
    const seconds = 0.6 + Math.random() * 0.8;
    await new Promise((r) => setTimeout(r, seconds * 1000));
    timings.push({ step: index, seconds, detail: null });
  }
  return { outcome: { status: "done" }, log: [], timings };
}

// ------------------------------------------------------------ project view

const DAY = 86400;
const disk = { free_bytes: 41.3 * GiB, total_bytes: 58 * GiB };
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

function svc(name: string, extra: Partial<ServiceView>): ServiceView {
  return { name, database: false, routed: false, running: null, image: null, build: null, depends_on: [], volumes: [], ...extra };
}

function structure(extra: Partial<StructureView>): StructureView {
  return {
    compose_project: null, compose_files: [], engine: "postgres", db_service: "db", db_user: "app",
    routing_mode: "env", routing_file: ".env.local", routing_variable: "DATABASE_URL", listen: null, restart: [],
    migrations_tool: "alembic", migrations_dirs: ["alembic/versions"], migrations_command: "docker compose run --rm api alembic upgrade head",
    seed: null, main_branch: "main",
    ...extra,
  };
}

function shopApi(): ProjectDetail {
  return {
    root: "/home/dev/code/shop-api", path: "~/code/shop-api", name: "shop-api", mode: "env",
    git_branch: "feature/refunds", active_db: "feature_payments_db", problem: null,
    warnings: ["merged_search_db is in the registry but not on the server (removed outside sidetrack?)"],
    disk,
    services: [
      svc("db", { database: true, running: true, image: "postgres:16-alpine", volumes: [{ kind: "volume", source: "pgdata", target: "/var/lib/postgresql/data" }] }),
      svc("api", { routed: true, running: true, build: "~/code/shop-api", depends_on: ["db", "redis"], volumes: [{ kind: "bind", source: "~/code/shop-api", target: "/app" }] }),
      svc("worker", { routed: true, running: false, build: "~/code/shop-api", depends_on: ["db", "redis"] }),
      svc("redis", { running: true, image: "redis:7-alpine", volumes: [{ kind: "volume", source: "redis-data", target: "/data" }] }),
    ],
    structure: structure({ compose_project: "shop-api", restart: ["api", "worker"], migrations_dirs: ["migrations/versions"] }),
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
      db("spike_cache_db", "spike/cache", "app_dev", 60, 1.9, {
        git_branch_exists: false,
        snapshots: [
          { db_name: "spike_cache_db__s1", seq: 1, label: null, size_bytes: 1.8 * GiB, created_at: now() - 58 * DAY, on_server: true },
          { db_name: "spike_cache_db__s2", seq: 2, label: "warm cache", size_bytes: 1.9 * GiB, created_at: now() - 55 * DAY, on_server: true },
        ],
      }),
      db("old_checkout_db", "feature/old-checkout", "app_dev", 50, 1.4, { git_branch_exists: false }),
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
    git_branch: "fix/invoice-rounding", active_db: "app_dev", problem: null, warnings: [], disk,
    services: [
      svc("db", { database: true, running: true, image: "postgres:17", volumes: [{ kind: "volume", source: "billing_pg", target: "/var/lib/postgresql/data" }] }),
      svc("app", { routed: true, running: true, build: "~/code/billing", depends_on: ["db"] }),
    ],
    structure: structure({ compose_project: "billing", routing_mode: "proxy", listen: "127.0.0.1:6543", restart: ["app"], migrations_tool: "django", migrations_dirs: ["invoices/migrations"], migrations_command: "python manage.py migrate" }),
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
    warnings: [], disk: null,
    services: [
      svc("db", { database: true, running: false, image: "postgres:16" }),
      svc("etl", { routed: true, running: false, build: "~/code/analytics-etl", depends_on: ["db"] }),
    ],
    structure: structure({ compose_project: null, restart: ["etl"] }),
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
    git_branch: "main", active_db: "app_dev", problem: null, warnings: [], disk,
    services: [
      svc("db", { database: true, running: true, image: "postgres:16-alpine" }),
      svc("auth", { routed: true, running: true, image: "ghcr.io/acme/auth:dev", depends_on: ["db"] }),
    ],
    structure: structure({ compose_project: "auth-service", routing_mode: "proxy", listen: "127.0.0.1:6544", restart: ["auth"] }),
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

// ------------------------------------------------------------- other plans

/** Fail like the planner does: the app shows the message as a plain sentence. */
const refuse = (message: string) => Promise.reject(new Error(message));

/** `naming::db_name_for_branch`, without the length and collision rules. */
const dbName = (branch: string) => `${branch.toLowerCase().replace(/[^a-z0-9]+/g, "_")}_db`;

/**
 * A plan shaped like the planner's for `op`, from the demo project's state:
 * proxy projects hold connections during a copy, env projects stop the app.
 */
export async function demoPlan(root: string, op: Op): Promise<Plan> {
  await new Promise((r) => setTimeout(r, 300));
  const d = details[root]?.();
  if (!d) return refuse(`no demo data for ${root}`);
  const proxy = d.mode === "proxy";
  const app = d.services.filter((s) => !s.database && s.running).map((s) => s.name);
  const active = d.db_branches.find((b) => b.active);
  const byName = (n: string) => d.db_branches.find((b) => b.db_name === n || b.git_branch === n);
  const pause = (db: string): Step[] => (proxy ? [{ type: "hold_connections", db }] : app.length ? [{ type: "stop_services", services: app }] : []);
  const unpause = (db: string): Step[] => (proxy ? [{ type: "release_connections", db }] : app.length ? [{ type: "start_services", services: app }] : []);
  const size = (n: string) => d.db_branches.flatMap((b) => [b, ...b.snapshots]).find((b) => b.db_name === n)?.size_bytes ?? 0;
  const plan = (kind: string, summary: string, steps: Step[], warnings: string[] = [], notes: string[] = []): Plan =>
    ({ kind, summary, steps, warnings, notes });

  switch (op.op) {
    case "switch":
      return refuse("demo switch plans come with All projects");

    case "create": {
      const git = op.git_branch;
      const name = dbName(git);
      if (op.empty) {
        return plan("create", `Create ${name} for ${git}`, [
          { type: "create_empty_database", name },
          { type: "register_branch", git_branch: git, db_name: name, parent_db: null },
        ], d.warnings);
      }
      const base = op.from ? byName(op.from) : d.db_branches.find((b) => b.git_branch === "main");
      if (!base) return refuse(`no DB branch, git branch or snapshot called ${op.from}`);
      const busy = base.active;
      return plan("create", `Create ${name} for ${git}`, [
        ...(busy ? pause(base.db_name) : []),
        { type: "clone_database", source: base.db_name, target: name, size_bytes: size(base.db_name) },
        { type: "register_branch", git_branch: git, db_name: name, parent_db: base.db_name },
        ...(busy ? unpause(base.db_name) : []),
      ], d.warnings, [
        op.from ? `copying ${base.db_name} as requested` : `${base.db_name} has every migration ${git} shares with main, so it's the closest base`,
        ...(op.from ? [] : [`1 migration(s) will run when you switch to ${git}`]),
      ]);
    }

    case "snapshot": {
      if (!active) return refuse("can't read the active database");
      const label = op.label;
      if (label && d.db_branches.some((b) => b.snapshots.some((s) => s.label === label))) {
        return refuse(`a snapshot labelled "${label}" already exists`);
      }
      const seq = Math.max(0, ...active.snapshots.map((s) => s.seq)) + 1;
      const snap = `${active.db_name}__s${seq}`;
      return plan("snapshot", `Snapshot ${active.db_name} as ${snap}`, [
        ...pause(active.db_name),
        { type: "clone_database", source: active.db_name, target: snap, size_bytes: size(active.db_name) },
        { type: "freeze_database", name: snap },
        { type: "register_snapshot", branch_db: active.db_name, db_name: snap, seq, label },
        ...unpause(active.db_name),
      ], d.warnings);
    }

    case "snapshot_remove": {
      const s = d.db_branches.flatMap((b) => b.snapshots).find((s) => s.db_name === op.name || s.label === op.name);
      if (!s) return refuse(`no DB branch or snapshot called ${op.name}`);
      return plan("snapshot_remove", `Delete snapshot ${s.db_name}`, [
        ...(s.on_server !== false ? [{ type: "drop_database", name: s.db_name } as Step] : []),
        { type: "forget_snapshot", db_name: s.db_name },
      ]);
    }

    case "reset": {
      if (!active) return refuse("can't read the active database");
      const source = op.to ?? (op.parent ? active.parent : active.snapshots[0]?.db_name ?? active.parent);
      if (!source) return refuse(`${active.db_name} has no snapshots and no parent to reset to`);
      const tmp = `${active.db_name}__tmp`;
      return plan("reset", `Reset ${active.db_name} to a fresh copy of ${source}`, [
        ...pause(active.db_name),
        { type: "clone_database", source, target: tmp, size_bytes: size(source) },
        { type: "drop_database", name: active.db_name },
        { type: "rename_database", from: tmp, to: active.db_name },
        { type: "record_migrations", db_name: active.db_name },
        ...unpause(active.db_name),
        { type: "touch_branch", db_name: active.db_name },
      ], d.warnings);
    }

    case "delete": {
      const b = byName(op.name);
      if (!b) return refuse(`no DB branch for ${op.name}`);
      if (b.active) return refuse(`${b.db_name} is the active database; switch to another branch first`);
      if (b.worktree) return refuse(`${b.db_name} is in use in the worktree at ${b.worktree}; switch that worktree to another branch first`);
      return plan("delete", `Delete ${b.db_name} (git branch ${b.git_branch})`, deleteSteps(b), b.adopted ? [`${b.db_name} existed before sidetrack (it was adopted at init)`] : []);
    }

    case "rename": {
      const b = byName(op.old);
      if (!b) return refuse(`no DB branch for ${op.old}`);
      const other = d.db_branches.find((o) => o.git_branch === op.new);
      if (other) return refuse(`${op.new} already has a DB branch (${other.db_name})`);
      const to = b.adopted ? b.db_name : dbName(op.new);
      const route: Step[] = proxy
        ? [{ type: "set_proxy_target", database: "app_dev", from_db: b.db_name, to_db: to }]
        : [{ type: "rewrite_env", file: ".env", variable: "DATABASE_URL", from_db: b.db_name, to_db: to }];
      const steps: Step[] = to === b.db_name ? [] : b.active
        ? [...(app.length ? [{ type: "stop_services", services: app } as Step] : []), { type: "rename_database", from: b.db_name, to }, ...route, ...(app.length ? [{ type: "start_services", services: app } as Step] : [])]
        : [{ type: "rename_database", from: b.db_name, to }];
      return plan("rename", to === b.db_name ? `Link ${b.db_name} to git branch ${op.new} (was ${b.git_branch})` : `Rename ${b.db_name} → ${to} for git branch ${op.new}`, [
        ...steps,
        { type: "relink_branch", db_name: b.db_name, new_db_name: to, new_git_branch: op.new },
      ], d.warnings, b.git_branch_exists ? [`this only renames the database; rename the git branch with \`git branch -m ${b.git_branch} ${op.new}\``] : []);
    }

    case "gc": {
      const steps: Step[] = [];
      const notes: string[] = [];
      let freed = 0;
      for (const b of d.db_branches) {
        if (b.active) continue;
        if (b.worktree) {
          notes.push(`${b.db_name} is kept: it's in use in the worktree at ${b.worktree}`);
          continue;
        }
        if (b.git_branch_exists && b.on_server !== false) continue;
        notes.push(`${b.db_name} (${b.git_branch}): ${b.on_server === false ? "database is missing" : "git branch deleted"}`);
        freed += (b.size_bytes ?? 0) + b.snapshots.reduce((n, s) => n + (s.size_bytes ?? 0), 0);
        steps.push(...deleteSteps(b));
      }
      if (freed > 0) notes.push(`frees about ${bytes(freed)}`);
      return plan("gc", "Clean up DB branches whose git branch is gone or merged", steps, [], notes);
    }
  }
}

/** `Planner::push_delete_branch`: snapshots first, then the branch; drops only what's on the server. */
function deleteSteps(b: DbBranchView): Step[] {
  return [
    ...b.snapshots.flatMap((s): Step[] => [
      ...(s.on_server !== false ? [{ type: "drop_database", name: s.db_name } as Step] : []),
      { type: "forget_snapshot", db_name: s.db_name },
    ]),
    ...(b.on_server !== false ? [{ type: "drop_database", name: b.db_name } as Step] : []),
    { type: "forget_branch", db_name: b.db_name },
  ];
}

// ------------------------------------------------------------- add project

let demoDbStarted = false;

/** A repo that isn't added yet, with its database stopped until "Start the database". */
export async function demoPreview(dir: string): Promise<AddPreview> {
  await new Promise((r) => setTimeout(r, 400));
  const toml = `[database]
engine = "postgres"
service = "db"
user = "app"

[routing]
mode = "env"
file = ".env"
variable = "DB_NAME"
restart = ["api", "flyway"]

[migrations]
tool = "flyway"
dirs = ["db/migration"]
command = "docker compose run --rm flyway migrate"

[git]
main_branch = "main"
`;
  return {
    root: dir,
    name: dir.split(/[\/]/).pop() ?? dir,
    branch: "main",
    registered: false,
    detected: {
      compose_project: "inventory",
      db_service: "db",
      db_image: "postgres:16-alpine",
      user: "app",
      active_db: "inventory_dev",
      app_services: ["api", "flyway"],
      routing: { file: ".env", variable: "DB_NAME", services: ["api", "flyway"] },
      routing_problem: null,
      migrations: { tool: "flyway", dirs: ["db/migration"], command: "docker compose run --rm flyway migrate", service: "flyway" },
      notes: [],
    },
    config: {
      database: { engine: "postgres", service: "db", user: "app" },
      routing: { mode: "env", file: ".env", variable: "DB_NAME", restart: ["api", "flyway"] },
      migrations: { tool: "flyway", dirs: ["db/migration"], command: "docker compose run --rm flyway migrate" },
      git: { main_branch: "main" },
    },
    toml,
    database_running: demoDbStarted,
    hook_installed: false,
  };
}

export async function demoStartDatabase(): Promise<void> {
  await new Promise((r) => setTimeout(r, 900));
  demoDbStarted = true;
}

export async function demoAdd(_root: string, hook: boolean): Promise<string[]> {
  await new Promise((r) => setTimeout(r, 700));
  demoDbStarted = true;
  return [
    "Wrote sidetrack.toml. It holds no secrets and is safe to commit.",
    "Linked main → inventory_dev (assuming it has every migration in this checkout).",
    ...(hook ? ["Hook installed in /home/dev/code/inventory/.git/hooks/post-checkout."] : []),
  ];
}
