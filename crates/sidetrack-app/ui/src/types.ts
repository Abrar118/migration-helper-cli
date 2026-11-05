// Mirrors of the JSON the Rust side sends. `Plan` and `Step` are sidetrack-core's
// stable plan format (`{"type": "<snake_case>", …}`).

export type Step =
  | { type: "stop_services"; services: string[] }
  | { type: "terminate_connections"; db: string; sessions: string[] }
  | { type: "clone_database"; source: string; target: string; size_bytes: number }
  | { type: "create_empty_database"; name: string }
  | { type: "freeze_database"; name: string }
  | { type: "drop_database"; name: string }
  | { type: "drop_temporary_database"; name: string }
  | { type: "rename_database"; from: string; to: string }
  | { type: "rewrite_env"; file: string; variable: string; from_db: string; to_db: string }
  | { type: "hold_connections"; db: string }
  | { type: "release_connections"; db: string }
  | { type: "set_env_port"; file: string; variable: string; port: number; copy_from: string | null }
  | { type: "set_proxy_target"; database: string; from_db: string; to_db: string }
  | { type: "start_services"; services: string[] }
  | { type: "run_migrations"; command: string; pending: string[] }
  | { type: "run_seed"; command: string }
  | { type: "register_branch"; git_branch: string; db_name: string; parent_db: string | null }
  | { type: "record_migrations"; db_name: string }
  | { type: "touch_branch"; db_name: string }
  | { type: "register_snapshot"; branch_db: string; db_name: string; seq: number; label: string | null }
  | { type: "forget_branch"; db_name: string }
  | { type: "forget_snapshot"; db_name: string }
  | { type: "relink_branch"; db_name: string; new_db_name: string; new_git_branch: string };

export interface TableChange {
  table: string;
  added_columns: string[];
  removed_columns: string[];
}

export interface SchemaDiff {
  from_db: string;
  to_db: string;
  added_tables: string[];
  removed_tables: string[];
  changed_tables: TableChange[];
}

export interface Plan {
  kind: string;
  summary: string;
  steps: Step[];
  warnings: string[];
  notes: string[];
  schema?: SchemaDiff;
}

export interface DiskSpace {
  free_bytes: number;
  total_bytes: number;
  /** Docker Desktop: the numbers are the host drive holding its VM disk. */
  host_limited?: boolean;
}

export interface MigrationStatus {
  db_name: string;
  source: string;
  applied: number;
  pending: string[];
  unknown: string[];
}

export interface ProjectView {
  root: string;
  path: string;
  name: string;
  mode: string;
  listen: string | null;
  proxy_running: boolean | null;
  git_branch: string | null;
  active_db: string | null;
  problem: string | null;
  branches: number;
  snapshots: number;
  running_services: string[];
  elsewhere: [string, string][];
  migrations: MigrationStatus | null;
  warnings: string[];
  plan: Plan | null;
}

export interface Overview {
  projects: ProjectView[];
  disk: DiskSpace | null;
  registry: string;
}

export type Outcome =
  | { status: "done" }
  | { status: "rolled_back"; step: string; error: string; undo_errors: string[] }
  | { status: "stopped"; step: string; error: string; note: string };

export interface Report {
  outcome: Outcome;
  log: string[];
  timings: { step: number; seconds: number; detail: string | null }[];
}

export interface Commit {
  id: string;
  subject: string;
  /** Unix seconds. */
  time: number;
}

export interface GitBranchView {
  name: string;
  db: string | null;
  here: boolean;
  worktree: string | null;
  commits: Commit[];
}

export interface SnapshotView {
  db_name: string;
  seq: number;
  label: string | null;
  size_bytes: number | null;
  created_at: number;
  on_server: boolean | null;
}

export interface DbBranchView {
  db_name: string;
  git_branch: string;
  parent: string | null;
  git_branch_exists: boolean;
  on_server: boolean | null;
  size_bytes: number | null;
  adopted: boolean;
  created_at: number;
  last_used_at: number;
  active: boolean;
  worktree: string | null;
  snapshots: SnapshotView[];
}

export interface VolumeView {
  kind: string | null;
  source: string | null;
  target: string | null;
}

export interface ServiceView {
  name: string;
  database: boolean;
  /** Restarted or re-pointed when the database changes: it reads the DB name. */
  routed: boolean;
  running: boolean | null;
  image: string | null;
  /** Build context, for a service built from the repo. */
  build: string | null;
  depends_on: string[];
  volumes: VolumeView[];
}

/** How the project is put together (`sidetrack.toml`, plus compose's project name). */
export interface StructureView {
  compose_project: string | null;
  compose_files: string[];
  engine: string;
  db_service: string;
  db_user: string;
  routing_mode: string;
  routing_file: string;
  routing_variable: string;
  listen: string | null;
  restart: string[];
  migrations_tool: string;
  migrations_dirs: string[];
  migrations_command: string;
  seed: string | null;
  main_branch: string | null;
}

/** One project's git branches and DB branches (`project_detail`). */
export interface ProjectDetail {
  root: string;
  path: string;
  name: string;
  mode: string;
  git_branch: string | null;
  active_db: string | null;
  problem: string | null;
  git_branches: GitBranchView[];
  db_branches: DbBranchView[];
  services: ServiceView[];
  structure: StructureView;
  warnings: string[];
  disk: DiskSpace | null;
}

/** `plan_op`'s answer: the plan, plus problems met while planning (the proxy wouldn't start). */
export interface Planned {
  plan: Plan;
  warnings: string[];
}

/** An operation to plan and apply (`ops::Op` on the Rust side), mirroring the CLI command. */
export type Op =
  | { op: "switch" }
  | { op: "create"; git_branch: string; from: string | null; empty: boolean }
  | { op: "snapshot"; label: string | null }
  | { op: "snapshot_remove"; name: string }
  | { op: "reset"; to: string | null; parent: boolean }
  | { op: "delete"; name: string }
  | { op: "rename"; old: string; new: string }
  | { op: "gc" };

/** What detection found in a repo's compose setup (`detect::Detection`). */
export interface Detection {
  compose_project: string;
  db_service: string;
  db_image: string;
  user: string | null;
  active_db: string;
  app_services: string[];
  routing: { file: string; variable: string; services: string[] } | null;
  routing_problem: string | null;
  inline: { service: string; variable: string }[];
  migrations: { tool: string; dirs: string[]; command: string | null; service: string | null };
  notes: string[];
}

/** The parts of `sidetrack.toml` the Add project sheet shows (`config::Config`). */
export interface ProjectConfig {
  database: { engine: string; service: string; user: string };
  routing: { mode: string; file: string; variable: string; restart: string[]; database?: string; listen?: string };
  compose?: { files?: string[] };
  migrations: { tool: string; dirs: string[]; command: string };
  git: { main_branch: string | null };
}

/** What adding a repo would do (`add::AddPreview`). */
export interface AddPreview {
  root: string;
  name: string;
  branch: string | null;
  registered: boolean;
  detected: Detection | null;
  config: ProjectConfig;
  toml: string;
  database_running: boolean;
  hook_installed: boolean;
  proxy_upstream: string | null;
  warnings: string[];
}
