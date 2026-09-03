'use client';

import { useEffect, useMemo, useRef, type RefObject } from 'react';
import { useApp } from '@/lib/store';
import { markFor, resolveTypes, slotFor } from '@/lib/scene/classification';
import { signatureBars, titleBox, titleSizeFor } from '@/lib/scene/lexicon';
import { lodFor } from '@/lib/scene/lod';
import type { Entry } from '@/lib/types';
import MarkGlyph from './MarkGlyph';
import type { CanvasRenderer } from './renderer';
import styles from './Canvas.module.css';

const SIGNATURE_MAX = 34;
const LABEL_CLEARANCE = 7;
const ISOLATED_ALPHA = 0.78;
const SEARCH_DIM = 0.16;
/** 0 first: keep the midpoint when it is already clear. */
const LABEL_STEPS = [0, 0.09, 0.16, 0.23, 0.3, 0.37];

/**
 * The lexicon map: titles are the nodes, so this layer is the canvas as far as
 * the reader is concerned. Everything inside a node is sized in em and the
 * camera drives one font-size, which keeps zooming to a single style write.
 */
export default function LabelOverlay({
  renderer,
}: {
  renderer: RefObject<CanvasRenderer | null>;
}) {
  const entries = useApp((s) => s.entries);
  const order = useApp((s) => s.order);
  const edges = useApp((s) => s.edges);
  const customTypes = useApp((s) => s.customTypes);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());

  const types = useMemo(() => resolveTypes(customTypes), [customTypes]);

  /** Connects to nothing (§5.3) — the blob carried this before the text did. */
  const connected = useMemo(() => {
    const ids = new Set<string>();
    for (const e of edges) {
      if (e.status === 'dismissed') continue;
      ids.add(e.entryA);
      ids.add(e.entryB);
    }
    return ids;
  }, [edges]);

  const nodes = useMemo(
    () =>
      order
        .map((id) => entries.get(id))
        .filter((e): e is Entry => !!e && e.parentEdge === null)
        .map((entry) => {
          const slot = slotFor(entry, types);
          const size = titleSizeFor(entry);
          const box = titleBox(entry, slot?.family ?? 'serif');
          const sigWidth = Math.min(box.w, SIGNATURE_MAX);
          return {
            entry,
            slot,
            size,
            box,
            mark: markFor(entry, types),
            isolated: !connected.has(entry.id),
            bars: signatureBars(entry.id, sigWidth).map((b) => ({
              x: b.x / size,
              w: b.w / size,
              h: b.h / size,
            })),
            sigWidth: sigWidth / size,
          };
        }),
    [order, entries, types, connected],
  );

  /**
   * A label pinned to the midpoint lands on a title often enough to be a
   * problem, so each one slides along its own edge to the nearest clear spot.
   * Computed once here rather than per frame; a drag can scuff it briefly.
   */
  const edgeLabels = useMemo(() => {
    const boxes = nodes.map(({ entry, box }) => ({
      x: entry.x,
      y: entry.y,
      halfW: box.w / 2 + LABEL_CLEARANCE,
      halfH: box.h / 2 + LABEL_CLEARANCE,
    }));
    const clear = (x: number, y: number) =>
      !boxes.some((n) => Math.abs(n.x - x) < n.halfW && Math.abs(n.y - y) < n.halfH);

    return edges
      .filter((e) => e.status !== 'dismissed')
      .map((edge) => {
        const a = entries.get(edge.entryA);
        const b = entries.get(edge.entryB);
        if (!a || !b) return null;
        let t = 0.5;
        for (const step of LABEL_STEPS) {
          const hit = [0.5 - step, 0.5 + step].find((c) =>
            clear(a.x + (b.x - a.x) * c, a.y + (b.y - a.y) * c),
          );
          if (hit !== undefined) {
            t = hit;
            break;
          }
        }
        return { edge, a, b, t };
      })
      .filter((v) => v !== null);
  }, [edges, entries, nodes]);

  useEffect(() => {
    const position = (camera: { zoom: number }) => {
      const r = renderer.current;
      if (!r) return;
      const lod = lodFor(camera.zoom);
      // §5.4 relations are the point, but twenty of them at once is a net, not
      // a map. They belong to the entry you are reading, so they appear with it.
      const hovered = useApp.getState().hoveredEntryId;
      // §5.3 — a search dims everything it did not match. This lived in the blob
      // fill before the canvas became text.
      const matched = useApp.getState().matchedIds;

      for (const { entry, size } of nodes) {
        const el = nodeRefs.current.get(`title:${entry.id}`);
        if (!el) continue;
        const w = r.positionOf(entry);
        const p = r.worldToScreen(w.x, w.y);
        el.style.fontSize = `${size * camera.zoom}px`;
        el.style.setProperty('--node-dim', matched && !matched.has(entry.id) ? String(SEARCH_DIM) : '1');
        el.style.transform = `translate(-50%, -50%) translate(${p.x}px, ${p.y}px)`;
        el.dataset.detail = lod.fingerprints ? 'on' : 'off';
      }

      for (const { edge, a, b, t } of edgeLabels) {
        const el = nodeRefs.current.get(`edge:${edge.id}`);
        if (!el) continue;
        const mine = hovered !== null && (edge.entryA === hovered || edge.entryB === hovered);
        if (!lod.edgeLabels || !mine) {
          el.style.visibility = 'hidden';
          continue;
        }
        const wa = r.positionOf(a);
        const wb = r.positionOf(b);
        const p = r.worldToScreen(wa.x + (wb.x - wa.x) * t, wa.y + (wb.y - wa.y) * t);
        el.style.visibility = 'visible';
        el.style.transform = `translate(-50%, -50%) translate(${p.x}px, ${p.y}px)`;
      }
    };

    const reposition = () => position(useApp.getState().camera);
    reposition();
    const offCamera = useApp.subscribe((s) => s.camera, position);
    const offDrag = useApp.subscribe((s) => s.dragging, reposition);
    const offHover = useApp.subscribe((s) => s.hoveredEntryId, reposition);
    const offMatched = useApp.subscribe((s) => s.matchedIds, reposition);
    return () => {
      offCamera();
      offDrag();
      offHover();
      offMatched();
    };
  }, [nodes, edgeLabels, renderer]);

  const setRef = (key: string) => (el: HTMLDivElement | null) => {
    if (el) nodeRefs.current.set(key, el);
    else nodeRefs.current.delete(key);
  };

  return (
    <div className={styles.overlay}>
      {nodes.map(({ entry, slot, box, mark, bars, sigWidth, isolated }) => (
        <div
          key={entry.id}
          ref={setRef(`title:${entry.id}`)}
          className={styles.node}
          style={{
            fontFamily: slot?.family === 'mono' ? 'var(--font-mono)' : 'var(--font-serif)',
            fontWeight: slot?.weight ?? 400,
            fontStyle: slot?.italic ? 'italic' : 'normal',
            letterSpacing: slot ? `${slot.tracking / 11}em` : undefined,
            ['--node-alpha' as string]: (slot?.opacity ?? 1) * (isolated ? ISOLATED_ALPHA : 1),
            color: isolated ? 'var(--dimmed-fg)' : undefined,
          } as React.CSSProperties}
        >
          <span className={styles.rail}>
            <MarkGlyph mark={mark} size="0.62em" />
          </span>
          <span className={styles.nodeTitle}>
            {box.lines.map((line, i) => (
              <span key={i} className={styles.nodeLine}>
                {line}
              </span>
            ))}
          </span>
          <svg
            className={styles.signature}
            width={`${sigWidth}em`}
            height="0.42em"
            viewBox={`0 0 ${sigWidth} 0.42`}
            preserveAspectRatio="xMinYMax meet"
            aria-hidden
          >
            {bars.map((b, i) => (
              <rect key={i} x={b.x} y={0.42 - b.h} width={b.w} height={b.h} fill="var(--meta)" opacity={0.75} />
            ))}
          </svg>
        </div>
      ))}
      {edgeLabels.map(({ edge }) => (
        <div key={edge.id} ref={setRef(`edge:${edge.id}`)} className={styles.edgeLabel}>
          {edge.relation}
        </div>
      ))}
    </div>
  );
}
