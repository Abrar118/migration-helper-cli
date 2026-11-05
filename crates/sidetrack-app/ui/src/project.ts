import { useCallback, useEffect, useRef, useState } from "react";
import { loadDetail, planOp } from "./api";
import { sentence } from "./text";
import type { DbBranchView, GitBranchView, Op, Plan, ProjectDetail } from "./types";

/** What's selected. A git branch and its DB branch are selected together. */
export type Selection = { git: string } | { db: string };

/** Where an action lives: a DB node, a git row, or the whole project (gc). */
export type Place = `db:${string}` | `git:${string}` | "project";

/** An inline field that's open: a snapshot label, a reset source or a rename target. */
export type Field = { kind: "snapshot" | "reset" | "rename"; db: string } | null;

/** The plan on screen instead of the tree, and the operation it came from. */
export interface Shown {
  op: Op;
  plan: Plan;
  title: string;
  /** Problems met while planning that aren't in the plan (the proxy wouldn't start). */
  warnings: string[];
}

/** What the planner said no to, shown where the action was chosen. */
export interface Refusal {
  place: Place;
  message: string;
}

/** The functions an action needs: plan an operation, open or close a field. */
export interface Actions {
  /** The action being planned right now, by `place` + label. */
  planning: string | null;
  refusal: Refusal | null;
  field: Field;
  setField: (f: Field) => void;
  show: (place: Place, key: string, op: Op, title: string) => void;
}

const message = (e: unknown) => sentence(e instanceof Error ? e.message : String(e));

/**
 * One project's state, shared by both looks: the detail read from the core, the
 * selection, the open field, and the plan on screen. Escape steps back one level
 * (proof, field, then `onBack`).
 */
export function useProject(root: string, onBack: () => void) {
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [sel, setSel] = useState<Selection | null>(null);
  const [field, setField] = useState<Field>(null);
  const [planning, setPlanning] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  const [shown, setShown] = useState<Shown | null>(null);
  const [applying, setApplying] = useState(false);
  const busy = useRef(false);
  const main = useRef<HTMLElement>(null);
  const shownRef = useRef(shown);
  shownRef.current = shown;

  const refresh = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setLoading(true);
    try {
      const d = await loadDetail(root);
      setDetail(d);
      setError(null);
      // Keep the selection while it still exists; else the checked-out branch.
      setSel((cur) => {
        if (cur && "git" in cur && d.git_branches.some((b) => b.name === cur.git)) return cur;
        if (cur && "db" in cur && d.db_branches.some((b) => b.db_name === cur.db)) return cur;
        const first = d.git_branches[0];
        return first ? { git: first.name } : d.db_branches[0] ? { db: d.db_branches[0].db_name } : null;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }, [root]);

  // Like All projects: read again whenever the window comes back to the front,
  // unless a plan is on screen (it stays as shown until it's closed).
  useEffect(() => {
    refresh();
    const onFocus = () => {
      if (!shownRef.current) refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  /** Back to the tree from a proof: read the project again, focus the selection. */
  const closeProof = useCallback(async () => {
    setShown(null);
    setApplying(false);
    await refresh();
    requestAnimationFrame(() => (document.querySelector(".node[aria-current='true'], .cnode[aria-current='true']") as HTMLElement | null)?.focus());
  }, [refresh]);

  // Escape steps back one level: proof → tree, open field → closed, tree → all projects.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (shown) {
        if (!applying) closeProof();
      } else if (field) {
        setField(null);
        setRefusal(null);
      } else {
        onBack();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack, shown, applying, field, closeProof]);

  const show = useCallback(
    async (place: Place, key: string, op: Op, title: string) => {
      setPlanning(`${place}/${key}`);
      setRefusal(null);
      try {
        const { plan, warnings } = await planOp(root, op);
        setShown({ op, plan, title, warnings });
        setField(null);
        // The tree may have been scrolled to its selection; a proof starts at the top, under its lead.
        main.current?.scrollTo(0, 0);
      } catch (e) {
        setRefusal({ place, message: message(e) });
      } finally {
        setPlanning(null);
      }
    },
    [root],
  );
  const actions: Actions = { planning, refusal, field, setField, show };

  // Picking another branch sets an unapplied plan aside; a running one stays.
  const select = (s: Selection) => {
    if (applying) return;
    setSel(s);
    setShown(null);
    setField(null);
    setRefusal(null);
  };

  const gitSel: GitBranchView | null = sel && "git" in sel ? detail?.git_branches.find((b) => b.name === sel.git) ?? null : null;
  const dbSel: DbBranchView | null =
    sel && "db" in sel
      ? detail?.db_branches.find((b) => b.db_name === sel.db) ?? null
      : gitSel?.db
        ? detail?.db_branches.find((b) => b.db_name === gitSel.db) ?? null
        : null;
  // A DB branch picked in the tree selects its git branch too, when it still exists.
  const gitShown = gitSel ?? (dbSel?.git_branch_exists ? detail?.git_branches.find((b) => b.name === dbSel.git_branch) ?? null : null);

  return { detail: detail as ProjectDetail | null, error, loading, refresh, sel, select, setSel, field, setField, planning, refusal, shown, applying, setApplying, show, closeProof, actions, main, gitSel, dbSel, gitShown };
}
