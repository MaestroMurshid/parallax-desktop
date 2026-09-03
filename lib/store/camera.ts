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
}

export const createCameraSlice: StateCreator<AppState, Mutators, [], CameraSlice> = (set, get) => ({
  camera: { x: 0, y: 0, zoom: 1 },

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
});
