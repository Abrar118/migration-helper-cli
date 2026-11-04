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

export interface ServiceView {
  name: string;
  database: boolean;
  running: boolean | null;
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
  warnings: string[];
}
