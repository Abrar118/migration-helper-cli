import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode, type WheelEvent } from "react";
import { loadJson, saveJson } from "./theme";

export interface CanvasNode {
  id: string;
  /** Where the layout puts it, in canvas pixels. A dragged node keeps its own spot. */
  x: number;
  y: number;
  w: number;
  /** Extra classes for its kind: `git`, `db`, `snap`, `svc`, … */
  kind: string;
  /** Its colour-coded type, named in the legend: `git`, `db`, `image`, `volume`, … */
  type?: string;
  /** Read by screen readers instead of the content. */
  label: string;
  selected?: boolean;
  /** Dimmed: shown for context only (a database that's not on the server). */
  muted?: boolean;
  children: ReactNode;
}

export interface CanvasEdge {
  from: string;
  to: string;
  /** `map` (git → db), `parent`, `snap`, `dep`, `reads`, `runs`, `mounts`… for styling. */
  kind: string;
  label?: string;
  /** `side` (default) runs right to left edges; `below` drops from the source's
      bottom-left (snapshots); `loop` bends out to the right (between nodes in one column). */
  route?: "side" | "below" | "loop";
}

/** What dropping a dragged node on a target does. */
export interface Drop {
  /** Shown while hovering: "Copy app_dev for fix/currency-format". */
  label: string;
  danger?: boolean;
  run: () => void;
}

/** A fixed place to drop a node on (the snapshot shelf, the delete bin). */
export interface Zone {
  id: string;
  label: string;
  danger?: boolean;
}

interface Props {
  /** Saves the layout under this key, e.g. `<root>:databases`. */
  layoutKey: string;
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  onSelect?: (id: string) => void;
  /** What dropping `dragged` on `target` (a node or zone id) would do, or null. */
  dropFor?: (dragged: string, target: string) => Drop | null;
  zones?: Zone[];
  /** Says what can be dragged where, under the canvas. */
  hint?: string;
  label: string;
  /** Shown under the selected node (its actions); moves with it. */
  card?: ReactNode;
  /** The node the card belongs to. */
  cardFor?: string | null;
  /** A click on the empty canvas (not a pan): closes the card. */
  onBackground?: () => void;
  /** What each node colour means; only the types on this canvas are listed. */
  legend?: { type: string; label: string }[];
}

type Pos = Record<string, [number, number]>;
interface View { x: number; y: number; z: number }

const PAD = 32;
const DRAG_START = 4;

/**
 * A pannable, zoomable canvas of HTML nodes joined by SVG edges. Nodes are
 * buttons: click selects, drag moves them (the spot is saved), and dropping one
 * on a target that accepts it runs that target's action instead of moving it.
 * Drag the background or scroll to pan; Ctrl+scroll zooms.
 */
export function Canvas({ layoutKey, nodes, edges, onSelect, dropFor, zones = [], hint, label, card, cardFor, onBackground, legend = [] }: Props) {
  const storeKey = `sidetrack.layout:${layoutKey}`;
  const [moved, setMoved] = useState<Pos>(() => loadJson<Pos>(storeKey, {}));
  const [view, setView] = useState<View>({ x: PAD, y: PAD, z: 1 });
  const [sizes, setSizes] = useState<Record<string, [number, number]>>({});
  const [keyOpen, setKeyOpen] = useState(() => loadJson("sidetrack.legend-open", true));
  const [drag, setDrag] = useState<{ id: string; dx: number; dy: number; over: string | null; drop: Drop | null } | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const world = useRef<HTMLDivElement>(null);
  const suppressClick = useRef(false);

  useEffect(() => setMoved(loadJson<Pos>(storeKey, {})), [storeKey]);

  const pos = useCallback(
    (n: CanvasNode): [number, number] => {
      const m = moved[n.id];
      const base: [number, number] = m ?? [n.x, n.y];
      return drag?.id === n.id ? [base[0] + drag.dx / view.z, base[1] + drag.dy / view.z] : base;
    },
    [moved, drag, view.z],
  );

  // Edges need each node's rendered height.
  useLayoutEffect(() => {
    const el = world.current;
    if (!el) return;
    const measure = () => {
      const next: Record<string, [number, number]> = {};
      el.querySelectorAll<HTMLElement>("[data-node-id]").forEach((n) => {
        next[n.dataset.nodeId!] = [n.offsetWidth, n.offsetHeight];
      });
      setSizes((cur) => (JSON.stringify(cur) === JSON.stringify(next) ? cur : next));
    };
    measure();
    const ro = new ResizeObserver(measure);
    el.querySelectorAll("[data-node-id]").forEach((n) => ro.observe(n));
    return () => ro.disconnect();
  }, [nodes]);

  const fit = useCallback(() => {
    const vp = viewport.current;
    if (!vp || nodes.length === 0) return;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const n of nodes) {
      const [x, y] = moved[n.id] ?? [n.x, n.y];
      const [w, h] = sizes[n.id] ?? [n.w, 60];
      x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x + w); y1 = Math.max(y1, y + h);
    }
    // Leave room on the right for the key.
    const reserve = keyOpen && legend.length > 0 ? 220 : 0;
    const z = Math.min(1, (vp.clientWidth - PAD * 2 - reserve) / (x1 - x0), (vp.clientHeight - PAD * 2) / (y1 - y0));
    setView({ z: Math.max(0.4, z), x: PAD - x0 * z, y: PAD - y0 * z });
  }, [nodes, moved, sizes, keyOpen, legend.length]);

  // On first view, zoom out to fit when the graph is bigger than the pane.
  const fitted = useRef<string | null>(null);
  useEffect(() => {
    const vp = viewport.current;
    if (!vp || fitted.current === layoutKey || Object.keys(sizes).length < nodes.length) return;
    fitted.current = layoutKey;
    const right = Math.max(...nodes.map((n) => (moved[n.id]?.[0] ?? n.x) + (sizes[n.id]?.[0] ?? n.w)));
    const bottom = Math.max(...nodes.map((n) => (moved[n.id]?.[1] ?? n.y) + (sizes[n.id]?.[1] ?? 60)));
    if (right + PAD * 2 + (keyOpen && legend.length > 0 ? 220 : 0) > vp.clientWidth || bottom + PAD * 2 > vp.clientHeight) fit();
  }, [layoutKey, nodes, sizes, moved, fit, keyOpen, legend.length]);

  const resetLayout = () => {
    setMoved({});
    saveJson(storeKey, null);
    setView({ x: PAD, y: PAD, z: 1 });
  };

  const zoomBy = (f: number, cx?: number, cy?: number) => {
    const vp = viewport.current;
    if (!vp) return;
    const r = vp.getBoundingClientRect();
    const px = cx ?? r.width / 2, py = cy ?? r.height / 2;
    setView((v) => {
      const z = Math.min(2, Math.max(0.4, v.z * f));
      return { z, x: px - ((px - v.x) * z) / v.z, y: py - ((py - v.y) * z) / v.z };
    });
  };

  const onWheel = (e: WheelEvent) => {
    if (e.ctrlKey || e.metaKey) {
      const r = viewport.current!.getBoundingClientRect();
      zoomBy(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientX - r.left, e.clientY - r.top);
    } else {
      setView((v) => ({ ...v, x: v.x - e.deltaX, y: v.y - e.deltaY }));
    }
  };

  // Ctrl+scroll must not zoom the whole web view.
  useEffect(() => {
    const vp = viewport.current;
    if (!vp) return;
    const stop = (e: globalThis.WheelEvent) => {
      if (e.ctrlKey || e.metaKey) e.preventDefault();
    };
    vp.addEventListener("wheel", stop, { passive: false });
    return () => vp.removeEventListener("wheel", stop);
  }, []);

  /** Panning: drag on the background. */
  const onBackgroundDown = (e: ReactPointerEvent) => {
    if (e.button !== 0 || (e.target as Element).closest("[data-node-id], .canvas-tools, .canvas-dock, .ccard, .canvas-legend")) return;
    const start = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    el.dataset.panning = "";
    let panned = false;
    const move = (ev: PointerEvent) => {
      if (Math.hypot(ev.clientX - start.x, ev.clientY - start.y) >= DRAG_START) panned = true;
      setView((v) => ({ ...v, x: start.vx + ev.clientX - start.x, y: start.vy + ev.clientY - start.y }));
    };
    const up = () => {
      if (!panned) onBackground?.();
      delete el.dataset.panning;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  /** Dragging a node: move it, or drop it on a target that takes it. */
  const onNodeDown = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0) return;
    const el = e.currentTarget as HTMLElement;
    const sx = e.clientX, sy = e.clientY;
    let started = false;
    let last: { dx: number; dy: number; over: string | null; drop: Drop | null } = { dx: 0, dy: 0, over: null, drop: null };
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - sx, dy = ev.clientY - sy;
      if (!started && Math.hypot(dx, dy) < DRAG_START) return;
      started = true;
      const hit = document
        .elementsFromPoint(ev.clientX, ev.clientY)
        .map((h) => h.closest<HTMLElement>("[data-node-id], [data-zone-id]"))
        .find((h) => h && h.dataset.nodeId !== id);
      const over = hit ? hit.dataset.nodeId ?? hit.dataset.zoneId ?? null : null;
      last = { dx, dy, over, drop: over && dropFor ? dropFor(id, over) : null };
      setDrag({ id, ...last });
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      if (!started) return;
      suppressClick.current = true;
      setDrag(null);
      if (last.drop) {
        last.drop.run();
        return;
      }
      setMoved((cur) => {
        const n = nodes.find((x) => x.id === id)!;
        const [bx, by] = cur[id] ?? [n.x, n.y];
        const next: Pos = { ...cur, [id]: [Math.round(bx + last.dx / view.z), Math.round(by + last.dy / view.z)] };
        saveJson(storeKey, next);
        return next;
      });
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };

  // Which nodes and zones would take the node being dragged.
  const takes = (target: string) => Boolean(drag && dropFor?.(drag.id, target));
  const cardBox = card && cardFor ? box(cardFor) : null;

  function box(id: string) {
    const n = nodes.find((x) => x.id === id);
    if (!n) return null;
    const [x, y] = pos(n);
    const [w, h] = sizes[id] ?? [n.w, 56];
    return { x, y, w, h };
  }

  return (
    <div className="canvas" ref={viewport} onPointerDown={onBackgroundDown} onWheel={onWheel} role="region" aria-label={label}>
      <div className="canvas-world" ref={world} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.z})` }}>
        <svg className="canvas-edges" aria-hidden="true">
          {edges.map((e) => {
            const a = box(e.from), b = box(e.to);
            if (!a || !b) return null;
            const p = path(a, b, e.route ?? "side");
            const on = nodes.some((n) => n.selected && (n.id === e.from || n.id === e.to));
            return (
              <g key={`${e.from}>${e.to}`} className={`edge edge-${e.kind}${on ? " on" : ""}`}>
                <path d={p.d} />
                {e.label && <text x={p.mx} y={p.my + 4} textAnchor="middle">{e.label}</text>}
              </g>
            );
          })}
        </svg>
        {nodes.map((n) => {
          const [x, y] = pos(n);
          const dragging = drag?.id === n.id;
          const target = drag && !dragging && takes(n.id);
          return (
            <button
              key={n.id}
              type="button"
              data-node-id={n.id}
              className={`cnode ${n.kind.split(" ").map((k) => `k-${k}`).join(" ")}${n.type ? ` t-${n.type}` : ""}${n.muted ? " muted" : ""}${dragging ? " dragging" : ""}${target ? " can-drop" : ""}${drag?.over === n.id && drag.drop ? " drop-over" : ""}`}
              style={{ left: x, top: y, width: n.w }}
              aria-current={n.selected ? "true" : undefined}
              aria-label={n.label}
              onPointerDown={(e) => onNodeDown(e, n.id)}
              onClick={() => {
                if (suppressClick.current) {
                  suppressClick.current = false;
                  return;
                }
                onSelect?.(n.id);
              }}
            >
              {n.children}
            </button>
          );
        })}
        {cardBox && !drag && (
          <div className="ccard" style={{ left: cardBox.x, top: cardBox.y + cardBox.h + 8 }} onPointerDown={(e) => e.stopPropagation()}>
            {card}
          </div>
        )}
      </div>

      {legend.some((l) => nodes.some((n) => n.type === l.type)) && (
        <div className="canvas-legend" aria-label="Colour key">
          <button type="button" aria-expanded={keyOpen} onClick={() => { setKeyOpen(!keyOpen); saveJson("sidetrack.legend-open", !keyOpen); }}>
            {keyOpen ? "Hide key" : "Show key"}
          </button>
          {keyOpen && <ul>
            {legend.filter((l) => nodes.some((n) => n.type === l.type)).map((l) => (
              <li key={l.type}><i className={`t-${l.type}`} aria-hidden="true" />{l.label}</li>
            ))}
          </ul>}
        </div>
      )}

      {zones.length > 0 && (
        <div className="canvas-dock" aria-label="Drop targets">
          {zones.map((z) => (
            <div
              key={z.id}
              data-zone-id={z.id}
              className={`zone${z.danger ? " danger" : ""}${drag ? (takes(z.id) ? " can-drop" : " off") : ""}${drag?.over === z.id && drag.drop ? " drop-over" : ""}`}
            >
              {z.label}
            </div>
          ))}
        </div>
      )}

      <div className="canvas-foot">
        <p className="canvas-hint" aria-live="polite">
          {drag ? (drag.drop ? <span className={drag.drop.danger ? "danger" : "go"}>Drop to plan: {drag.drop.label}</span> : "Drop anywhere to move it") : hint}
        </p>
        <div className="canvas-tools">
          <button type="button" onClick={() => zoomBy(1 / 1.2)} aria-label="Zoom out">−</button>
          <span className="zoom">{Math.round(view.z * 100)}%</span>
          <button type="button" onClick={() => zoomBy(1.2)} aria-label="Zoom in">+</button>
          <button type="button" onClick={fit}>Fit</button>
          <button type="button" onClick={resetLayout}>Reset layout</button>
        </div>
      </div>
    </div>
  );
}

/** The edge's path and its midpoint (for a label). */
function path(a: { x: number; y: number; w: number; h: number }, b: { x: number; y: number; w: number; h: number }, route: "side" | "below" | "loop") {
  if (route === "below") {
    const sx = a.x + 14, sy = a.y + a.h, ex = b.x, ey = b.y + b.h / 2;
    return { d: `M${sx} ${sy} V${ey - 6} Q${sx} ${ey} ${sx + 6} ${ey} H${ex}`, mx: (sx + ex) / 2, my: ey };
  }
  if (route === "loop") {
    const sx = a.x + a.w, sy = a.y + a.h / 2, ex = b.x + b.w, ey = b.y + b.h / 2;
    const out = Math.max(sx, ex) + 34;
    return { d: `M${sx} ${sy} C${out} ${sy}, ${out} ${ey}, ${ex} ${ey}`, mx: (sx + ex + 6 * out) / 8, my: (sy + ey) / 2 };
  }
  const forward = b.x >= a.x + a.w / 2;
  const sx = forward ? a.x + a.w : a.x, sy = a.y + a.h / 2;
  const ex = forward ? b.x : b.x + b.w, ey = b.y + b.h / 2;
  const c = Math.max(40, Math.abs(ex - sx) / 2) * (forward ? 1 : -1);
  return { d: `M${sx} ${sy} C${sx + c} ${sy}, ${ex - c} ${ey}, ${ex} ${ey}`, mx: (sx + ex) / 2, my: (sy + ey) / 2 };
}
