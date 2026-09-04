'use client';

import { useEffect, useRef, useState } from 'react';
import { useApp } from '@/lib/store';
import { titleBox } from '@/lib/scene/lexicon';
import { LOD } from '@/lib/scene/lod';
import { CanvasRenderer, type SceneEntry, type SceneInput } from './renderer';
import LabelOverlay from './LabelOverlay';
import Legend from './Legend';
import styles from './Canvas.module.css';

/** Pointer travel past which a press is a pan, not a click. */
const DRAG_THRESHOLD = 4;
/** Breathing room between an entry brought into the clear and the sheet edge. */
const EDGE_MARGIN = 72;

function buildScene(state: ReturnType<typeof useApp.getState>): SceneInput {
  const entries: SceneEntry[] = [];
  for (const id of state.order) {
    const entry = state.entries.get(id);
    // Children are layers on their parent's rings, never their own blob (§6.2).
    if (!entry || entry.parentEdge !== null) continue;
    entries.push({
      entry,
      box: titleBox(entry),
      returns: state.returnsFor(id),
      hasUnansweredQuestion: state.hasUnansweredQuestion(id),
      isolated: state.isIsolated(id),
    });
  }
  return { entries, edges: state.edges };
}

/**
 * The legend, the top bar and the bottom pills are absolutely positioned over
 * the canvas rather than beside it, so the drawable area and the *visible* area
 * are not the same rectangle. Framing against the full width centres the corpus
 * under the legend and pushes the left of it off screen.
 *
 * Read from the same custom properties the chrome is laid out with, so moving a
 * pill cannot silently desync the framing.
 */
function visibleInset(host: HTMLElement): { top: number; right: number; bottom: number } {
  const css = getComputedStyle(host);
  const px = (name: string, fallback: number) => {
    const v = parseFloat(css.getPropertyValue(name));
    return Number.isFinite(v) ? v : fallback;
  };
  return {
    top: px('--chrome-top', 58),
    right: px('--legend-w', 172) + px('--pill-inset', 10),
    bottom: px('--chrome-bottom', 50),
  };
}

/**
 * What the entry sheet covers while it is open (sec 6). The field keeps drawing
 * underneath it, so an entry in this strip cannot be read, hovered or dragged —
 * and placement does not know a sheet is about to open over what you just said.
 */
function sheetWidth(host: HTMLElement): number {
  const css = getComputedStyle(host);
  const px = (name: string, fallback: number) => {
    const v = parseFloat(css.getPropertyValue(name));
    return Number.isFinite(v) ? v : fallback;
  };
  return px('--entry-sheet-w', 560) + px('--pill-inset', 10) * 2;
}

/** Frame the whole corpus on first load; positions are frozen so this runs once. */
function fitToContent(
  scene: SceneInput,
  width: number,
  height: number,
  inset: { top: number; right: number; bottom: number },
) {
  if (scene.entries.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { entry, box } of scene.entries) {
    minX = Math.min(minX, entry.x - box.w / 2);
    minY = Math.min(minY, entry.y - box.h / 2);
    maxX = Math.max(maxX, entry.x + box.w / 2);
    maxY = Math.max(maxY, entry.y + box.h / 2);
  }
  const pad = 90;
  const availW = Math.max(1, width - inset.right);
  const availH = Math.max(1, height - inset.top - inset.bottom);
  const fit = Math.min(availW / (maxX - minX + pad), availH / (maxY - minY + pad));
  // Open at a zoom where titles are legible (§5.5) even if that leaves some of
  // the map off screen — a first view of unlabelled blobs reads as broken.
  const zoom = Math.min(1.4, Math.max(LOD.labels, fit));
  // Centre of the corpus over the centre of what can actually be seen, not of
  // the canvas: half the chrome's footprint, back in world units.
  return {
    x: (minX + maxX) / 2 + inset.right / 2 / zoom,
    y: (minY + maxY) / 2 + (inset.top - inset.bottom) / 2 / zoom,
    zoom,
  };
}

export default function Canvas() {
  const hostRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<CanvasRenderer | null>(null);
  const [ready, setReady] = useState(false);
  const hasEntries = useApp((s) => s.order.length > 0);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const renderer = new CanvasRenderer({ container: host });
    rendererRef.current = renderer;
    let disposed = false;
    let fitted = false;

    const unsubscribers: Array<() => void> = [];

    void renderer.init().then(() => {
      if (disposed) return;
      renderer.setScene(buildScene(useApp.getState()));
      renderer.setCamera(useApp.getState().camera);
      setReady(true);

      // Transient subscriptions: the renderer drives itself, no React re-render.
      unsubscribers.push(
        useApp.subscribe((s) => s.camera, (camera) => renderer.setCamera(camera)),
        useApp.subscribe((s) => s.theme, () => renderer.refreshTokens()),
        useApp.subscribe((s) => s.hoveredEntryId, (id) => {
          renderer.setHovered(id);
          // Imperative so hovering never re-renders; 'move' distinguishes
          // dragging a blob from panning the field (§5.1).
          host.style.cursor = id ? 'move' : 'grab';
        }),
        useApp.subscribe((s) => s.selectedEntryId, (id) => renderer.setSelected(id)),
        // Slide the field out from under the sheet when what you opened is
        // behind it. Only when it actually is: opening an entry you can already
        // see should not move the map you are reading it against.
        useApp.subscribe(
          (s) => [s.overlay, s.selectedEntryId] as const,
          ([overlay, id]) => {
            if (overlay !== 'entry' || !id) return;
            const entry = useApp.getState().entries.get(id);
            if (!entry) return;
            const rect = host.getBoundingClientRect();
            const clear = rect.width - sheetWidth(host) - EDGE_MARGIN;
            const p = renderer.worldToScreen(entry.x, entry.y);
            if (p.x <= clear) return;
            useApp.getState().panBy(clear - p.x, 0);
          },
        ),
        useApp.subscribe((s) => s.dragging, (d) => renderer.setDragging(d)),
        useApp.subscribe((s) => s.connecting, (c) => renderer.setConnecting(c)),
        useApp.subscribe(
          (s) => [s.matchedIds, s.matchedEdgeIds] as const,
          ([ids, edgeIds]) => renderer.setMatched(ids, edgeIds),
        ),
        useApp.subscribe(
          (s) => [s.entries, s.edges, s.questions] as const,
          () => {
            const scene = buildScene(useApp.getState());
            renderer.setScene(scene);
            if (fitted || scene.entries.length === 0) return;
            fitted = true;
            const rect = host.getBoundingClientRect();
            const fit = fitToContent(scene, rect.width, rect.height, visibleInset(host));
            if (fit) useApp.getState().setCamera(fit);
          },
        ),
      );
    });

    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      renderer.resize(width, height);
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      for (const off of unsubscribers) off();
      renderer.destroy();
      rendererRef.current = null;
    };
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !ready) return;

    let pointerId: number | null = null;
    let lastX = 0;
    let lastY = 0;
    let travel = 0;
    /** Set when the press landed on a blob: that blob moves instead of the camera. */
    let grabbed: { id: string; offsetX: number; offsetY: number } | null = null;
    let connectingFrom: string | null = null;

    const capture = (e: PointerEvent) => {
      try { host.setPointerCapture(e.pointerId); } catch { /* not capturable */ }
    };
    const release = (e: PointerEvent) => {
      try { host.releasePointerCapture(e.pointerId); } catch { /* never captured */ }
    };

    const local = (e: PointerEvent) => {
      const rect = host.getBoundingClientRect();
      return { x: e.clientX - rect.left, y: e.clientY - rect.top };
    };

    const onPointerDown = (e: PointerEvent) => {
      const renderer = rendererRef.current;
      if (!renderer) return;
      pointerId = e.pointerId;
      lastX = e.clientX;
      lastY = e.clientY;
      travel = 0;

      const { x, y } = local(e);
      // The handle is a separate target, so drag keeps meaning "move" (§5.1).
      const handleId = renderer.handleHitTest(x, y);
      if (handleId) {
        const world = renderer.screenToWorld(x, y);
        connectingFrom = handleId;
        useApp.getState().setConnecting({ fromId: handleId, x: world.x, y: world.y });
        capture(e);
        return;
      }

      const id = renderer.hitTest(x, y);
      if (id) {
        const entry = useApp.getState().entries.get(id);
        const world = renderer.screenToWorld(x, y);
        // Grab by the point you actually clicked, so the blob doesn't jump.
        grabbed = entry
          ? { id, offsetX: entry.x - world.x, offsetY: entry.y - world.y }
          : null;
      } else {
        grabbed = null;
      }
      host.style.cursor = grabbed ? 'move' : 'grabbing';
      capture(e);
    };

    const onPointerMove = (e: PointerEvent) => {
      const renderer = rendererRef.current;
      if (!renderer) return;

      if (connectingFrom) {
        const { x, y } = local(e);
        const world = renderer.screenToWorld(x, y);
        useApp.getState().setConnecting({ fromId: connectingFrom, x: world.x, y: world.y });
        return;
      }

      if (pointerId === e.pointerId) {
        const dx = e.clientX - lastX;
        const dy = e.clientY - lastY;
        travel += Math.abs(dx) + Math.abs(dy);
        lastX = e.clientX;
        lastY = e.clientY;
        if (travel <= DRAG_THRESHOLD) return;

        if (grabbed) {
          const { x, y } = local(e);
          const world = renderer.screenToWorld(x, y);
          useApp.getState().setDragging({
            id: grabbed.id,
            x: world.x + grabbed.offsetX,
            y: world.y + grabbed.offsetY,
          });
        } else {
          useApp.getState().panBy(dx, dy);
        }
        return;
      }

      const { x, y } = local(e);
      // Hover has to survive the trip out to the handle, or reaching for it
      // un-hovers the node and the handle vanishes from under the cursor.
      useApp.getState().setHovered(renderer.hitTest(x, y) ?? renderer.handleHitTest(x, y));
    };

    const onPointerUp = (e: PointerEvent) => {
      const renderer = rendererRef.current;
      if (!renderer) return;

      if (connectingFrom) {
        release(e);
        const { x, y } = local(e);
        const target = renderer.hitTest(x, y);
        const from = connectingFrom;
        connectingFrom = null;
        pointerId = null;
        useApp.getState().setConnecting(null);
        // Dropped on empty space, or on itself: nothing happens, no dialog.
        if (target && target !== from) useApp.getState().setPendingLink({ fromId: from, toId: target });
        return;
      }

      if (pointerId !== e.pointerId) return;
      release(e);
      pointerId = null;

      const state = useApp.getState();
      host.style.cursor = state.hoveredEntryId ? 'move' : 'grab';
      const moved = travel > DRAG_THRESHOLD;
      const drag = state.dragging;
      grabbed = null;

      if (moved) {
        // Commit once, on release (§5.1) — the drag itself never wrote to the corpus.
        if (drag) {
          void state.moveEntry(drag.id, drag.x, drag.y).finally(() => {
            useApp.getState().setDragging(null);
          });
        }
        return;
      }

      state.setDragging(null);
      const { x, y } = local(e);
      const id = renderer.hitTest(x, y);
      if (id) {
        state.openEntry(id);
        // Clicking an entry plays it back; typed entries are ignored (§8).
        state.playEntry(id);
      }
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const { x, y } = local(e as unknown as PointerEvent);
      const rect = host.getBoundingClientRect();
      useApp.getState().zoomAt(Math.exp(-e.deltaY * 0.0015), x, y, rect.width, rect.height);
    };

    host.addEventListener('pointerdown', onPointerDown);
    host.addEventListener('pointermove', onPointerMove);
    host.addEventListener('pointerup', onPointerUp);
    host.addEventListener('wheel', onWheel, { passive: false });

    return () => {
      host.removeEventListener('pointerdown', onPointerDown);
      host.removeEventListener('pointermove', onPointerMove);
      host.removeEventListener('pointerup', onPointerUp);
      host.removeEventListener('wheel', onWheel);
    };
  }, [ready]);

  return (
    <>
      <div className={styles.host} ref={hostRef}>
        {ready && <LabelOverlay renderer={rendererRef} />}
      </div>
      {ready && hasEntries && <Legend />}
    </>
  );
}

