import type { StateCreator } from 'zustand';
import { clampZoom } from '@/lib/scene/lod';
import type { AppState, Mutators } from './index';

/**
 * Camera is deliberately never read by a component during render — Pixi
 * subscribes transiently (useApp.subscribe) so panning at 120fps costs zero
 * React renders. Label overlay renders the visible set only; refs position them.
 */
export interface Camera {
  x: number;
  y: number;
  zoom: number;
}

export interface CameraSlice {
  camera: Camera;
  setCamera(next: Partial<Camera>): void;
  panBy(dx: number, dy: number): void;
  /** Zoom about a screen point so the world position under the cursor stays put. */
  zoomAt(factor: number, screenX: number, screenY: number, width: number, height: number): void;
  centerOn(x: number, y: number, zoom?: number): void;
  /**
   * The canvas element's own size, published by its ResizeObserver. The window
   * is not the canvas — a top bar, a bottom strip and the legend sit over it —
   * so fitting to `window.innerWidth` puts the outermost notes under the
   * chrome. `zoomAt` already takes the element's rect; this is the same number,
   * kept here because the fit is invoked from the strip, which has no ref to it.
   */
  viewport: { width: number; height: number };
  setViewport(width: number, height: number): void;
  /**
   * The only rescue for a note stranded off-screen (§5.1 forbids re-solving
   * the field, so the camera is the sole legal remedy). Reads position only —
   * never touches an entry's x/y.
   */
  fitAll(): void;
}

export const createCameraSlice: StateCreator<AppState, Mutators, [], CameraSlice> = (set, get) => ({
  camera: { x: 0, y: 0, zoom: 1 },
  viewport: { width: 0, height: 0 },

  setViewport(width, height) {
    set({ viewport: { width, height } });
  },

  setCamera(next) {
    const cam = get().camera;
    set({ camera: { ...cam, ...next, zoom: clampZoom(next.zoom ?? cam.zoom) } });
  },

  panBy(dx, dy) {
    const { x, y, zoom } = get().camera;
    set({ camera: { x: x - dx / zoom, y: y - dy / zoom, zoom } });
  },

  zoomAt(factor, screenX, screenY, width, height) {
    const cam = get().camera;
    const zoom = clampZoom(cam.zoom * factor);
    if (zoom === cam.zoom) return;
    // World point under the cursor before the zoom...
    const wx = cam.x + (screenX - width / 2) / cam.zoom;
    const wy = cam.y + (screenY - height / 2) / cam.zoom;
    // ...pinned there after it.
    set({ camera: { zoom, x: wx - (screenX - width / 2) / zoom, y: wy - (screenY - height / 2) / zoom } });
  },

  centerOn(x, y, zoom) {
    const cam = get().camera;
    set({ camera: { x, y, zoom: clampZoom(zoom ?? cam.zoom) } });
  },

  fitAll() {
    const { entries, order, viewport } = get();
    const { width, height } = viewport;
    // Nothing has measured the canvas yet, so any zoom computed here would be
    // Infinity. The button is unreachable before first paint anyway.
    if (width <= 0 || height <= 0) return;
    if (order.length === 0) {
      get().setCamera({ x: 0, y: 0, zoom: 1 });
      return;
    }

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const id of order) {
      const e = entries.get(id);
      if (!e) continue;
      minX = Math.min(minX, e.x);
      minY = Math.min(minY, e.y);
      maxX = Math.max(maxX, e.x);
      maxY = Math.max(maxY, e.y);
    }

    // Every id in `order` missing from `entries` would leave the bounds at
    // Infinity and produce a negative box. Treat it as nothing to fit.
    if (!Number.isFinite(minX) || !Number.isFinite(minY)) return;

    const boxWidth = maxX - minX;
    const boxHeight = maxY - minY;
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;

    // A single note (or several stacked at one point) makes the box zero-size
    // in one or both axes — dividing for zoom would be by zero. Keep the
    // current zoom and just recentre; there is nothing to fit around.
    if (boxWidth === 0 || boxHeight === 0) {
      get().setCamera({ x: cx, y: cy });
      return;
    }

    const MARGIN = 0.85;
    const zoom = Math.min(width / boxWidth, height / boxHeight) * MARGIN;
    get().setCamera({ x: cx, y: cy, zoom });
  },
});
