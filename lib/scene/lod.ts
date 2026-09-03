/**
 * Level of detail (§5.5). Zooming out drops labels → fingerprints → edges;
 * zooming in resolves the reverse. Canvas queries SQLite for the viewport
 * (positions, sizes, ids, titles) — transcripts never load into JS for this.
 */

export const LOD = {
  /** Below this, blobs and sizes only. */
  edges: 0.34,
  fingerprints: 0.62,
  labels: 0.85,
} as const;

export interface Lod {
  edges: boolean;
  edgeLabels: boolean;
  fingerprints: boolean;
  labels: boolean;
}

export function lodFor(zoom: number): Lod {
  return {
    edges: zoom >= LOD.edges,
    // An edge label is unreadable well before the edge itself is, so it needs
    // its own, higher bar (§5.4 keeps them horizontal for the same reason).
    edgeLabels: zoom >= LOD.fingerprints,
    fingerprints: zoom >= LOD.fingerprints,
    labels: zoom >= LOD.labels,
  };
}

export const ZOOM_MIN = 0.12;
export const ZOOM_MAX = 3.2;

export function clampZoom(z: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z));
}

/** Viewport query in world space — the shape the SQLite query will take. */
export interface Viewport {
  x: number;
  y: number;
  width: number;
  height: number;
  zoom: number;
}

export function visibleBounds(v: Viewport, margin = 120): { minX: number; minY: number; maxX: number; maxY: number } {
  const halfW = v.width / (2 * v.zoom) + margin;
  const halfH = v.height / (2 * v.zoom) + margin;
  return { minX: v.x - halfW, minY: v.y - halfH, maxX: v.x + halfW, maxY: v.y + halfH };
}
