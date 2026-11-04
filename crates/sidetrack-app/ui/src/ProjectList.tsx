import type { KeyboardEvent } from "react";
import { createsDatabase, target } from "./steps";
import { sentence } from "./text";
import type { ProjectView } from "./types";

/** One plain sentence of state for a project row. */
export function stateLine(p: ProjectView): { text: string; problem: boolean } {
  if (p.problem) return { text: sentence(p.problem.split(";")[0]), problem: true };
  if (!p.git_branch) return { text: "Detached HEAD, so there's nothing to compare", problem: false };
  if (p.plan) {
    return createsDatabase(p.plan)
      ? { text: `No database for ${p.git_branch} yet · plan ready`, problem: false }
      : { text: `${p.git_branch} will use ${target(p.plan) ?? "another database"} · plan ready`, problem: false };
  }
  const parts = [`${p.git_branch} is on ${p.active_db ?? "?"}`, "in sync"];
  if (p.elsewhere.length > 0) parts.push(`${p.elsewhere.length + 1} worktrees`);
  return { text: parts.join(" · "), problem: false };
}

interface Props {
  projects: ProjectView[];
  selected: string | null;
  onSelect: (root: string) => void;
}

export function ProjectList({ projects, selected, onSelect }: Props) {
  function onKey(e: KeyboardEvent<HTMLUListElement>) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const i = projects.findIndex((p) => p.root === selected);
    const next = projects[Math.min(projects.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))];
    if (next) {
      onSelect(next.root);
      document.getElementById(`project-${next.root}`)?.focus();
    }
  }

  return (
    <ul className="list" onKeyDown={onKey}>
      {projects.map((p) => {
        const s = stateLine(p);
        return (
          <li key={p.root}>
            <button
              id={`project-${p.root}`}
              className="row-button"
              aria-current={p.root === selected ? "true" : undefined}
              onClick={() => onSelect(p.root)}
            >
              <span className="name">{p.name}</span>
              <span className={`state${s.problem ? " problem" : ""}`}>{s.text}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
