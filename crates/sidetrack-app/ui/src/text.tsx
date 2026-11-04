import { Fragment } from "react";

/** A typed slug line: items separated by dots, each kept on one line, never cut off. */
export function Slug({ items, className = "" }: { items: (string | null | false | undefined)[]; className?: string }) {
  const shown = items.filter((i): i is string => Boolean(i));
  return (
    <p className={`slug ${className}`}>
      {shown.map((item, i) => (
        // The separator sits outside the nowrap span, so lines can break between items.
        <Fragment key={item}>
          <span className="nw">{item}</span>
          {i < shown.length - 1 && " · "}
        </Fragment>
      ))}
    </p>
  );
}

/** Core messages mark commands with backticks; set those in the code face. */
export function Code({ text }: { text: string }) {
  const parts = text.split("`");
  return (
    <>
      {parts.map((p, i) => (i % 2 === 1 ? <code key={i}>{p}</code> : <span key={i}>{p}</span>))}
    </>
  );
}

/**
 * Core messages start lowercase ("the db service isn't running"); start a sentence.
 * A leading identifier ("merged_search_db is…") keeps its case.
 */
export function sentence(s: string): string {
  const first = s.split(/\s/, 1)[0];
  return /^[a-z]+$/.test(first) ? s.charAt(0).toUpperCase() + s.slice(1) : s;
}
