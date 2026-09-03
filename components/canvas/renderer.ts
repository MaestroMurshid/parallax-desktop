/**
 * Canvas renderer. Subscribes to the store transiently — React never re-renders
 * to pan (no @pixi/react). In lexicon the nodes are text, so Pixi is down to
 * edges and the hover ring; LabelOverlay owns everything else.
 */

import { Application, Container, Graphics } from 'pixi.js';
import type { Edge, Entry } from '@/lib/types';
import { lodFor, type Lod } from '@/lib/scene/lod';
import type { TitleBox } from '@/lib/scene/lexicon';
import { readTokens, type Tokens } from '@/lib/scene/tokens';
import type { Camera } from '@/lib/store';

export interface SceneEntry {
  entry: Entry;
  /** World-space text box — in lexicon the title is the node. */
  box: TitleBox;
  returns: number;
  hasUnansweredQuestion: boolean;
  isolated: boolean;
}

export interface SceneInput {
  entries: SceneEntry[];
  edges: Edge[];
}

export interface RendererOptions {
  container: HTMLElement;
}

const DASH_SEGMENTS = 14;
const DASH_RATIO = 0.55;
/** Alpha multiplier for blobs outside an active search filter. */
const HIT_PAD = 4;
/** The connect handle: a stub at the title's right edge, hover only (§5.4). */
const HANDLE_LEN = 7;
/** titleBox estimates text width and runs narrow, so clear the real glyphs. */
const HANDLE_GAP = 16;
const HANDLE_HIT = 10;
const EDGE_GAP = 4;

/** Where the segment from `from` toward `to` leaves `from`'s text box. */
function clipToBox(
  from: { x: number; y: number },
  to: { x: number; y: number },
  box: TitleBox,
): { x: number; y: number } {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const len = Math.hypot(dx, dy);
  if (len === 0) return from;
  const halfW = box.w / 2 + EDGE_GAP;
  const halfH = box.h / 2 + EDGE_GAP;
  const tx = dx === 0 ? Infinity : halfW / Math.abs(dx);
  const ty = dy === 0 ? Infinity : halfH / Math.abs(dy);
  const t = Math.min(tx, ty);
  if (t >= 1) return to;
  return { x: from.x + dx * t, y: from.y + dy * t };
}

export class CanvasRenderer {
  private app = new Application();
  private tokens!: Tokens;

  private world = new Container();
  private edgeLayer = new Graphics();
  private highlightLayer = new Graphics();

  private scene: SceneInput = { entries: [], edges: [] };
  private byId = new Map<string, SceneEntry>();
  private camera: Camera = { x: 0, y: 0, zoom: 1 };
  private lod: Lod = lodFor(1);
  private hoveredId: string | null = null;
  private selectedId: string | null = null;
  private dragging: { id: string; x: number; y: number } | null = null;
  /** Active search filter; null means nothing dims. */
  private matched: Set<string> | null = null;
  private matchedEdges: Set<string> | null = null;
  /** In-flight manual connection: from a node, to wherever the cursor is. */
  private connecting: { fromId: string; x: number; y: number } | null = null;

  private width = 0;
  private height = 0;
  private destroyed = false;
  private initialized = false;

  constructor(private opts: RendererOptions) {}

  async init(): Promise<void> {
    this.tokens = readTokens();

    const rect = this.opts.container.getBoundingClientRect();
    this.width = rect.width;
    this.height = rect.height;

    await this.app.init({
      // Transparent: the page ground is token-driven CSS, so the canvas can't
      // drift from it on a theme switch.
      backgroundAlpha: 0,
      width: this.width,
      height: this.height,
      antialias: true,
      // Small geometry at 11px-adjacent scales needs real device pixels.
      resolution: window.devicePixelRatio || 1,
      autoDensity: true,
      preference: 'webgl',
    });
    this.initialized = true;
    // Strict mode can unmount before init resolves; tear down rather than attach.
    if (this.destroyed) {
      this.teardown();
      return;
    }

    this.opts.container.appendChild(this.app.canvas);

    this.world.addChild(this.edgeLayer, this.highlightLayer);
    this.app.stage.addChild(this.world);
    this.applyCamera();
  }

  /** Palette changed under us (theme switch); tokens are read from CSS. */
  refreshTokens(): void {
    this.tokens = readTokens();
    if (!this.initialized) return;
    this.draw();
  }

  destroy(): void {
    this.destroyed = true;
    if (this.initialized) this.teardown();
  }

  private teardown(): void {
    this.initialized = false;
    this.app.destroy(true, { children: true });
  }

  // -- inputs from the store ---------------------------------------------

  setScene(scene: SceneInput): void {
    this.scene = scene;
    this.byId = new Map(scene.entries.map((s) => [s.entry.id, s]));
    this.draw();
  }

  /**
   * Called on every camera change. Cheap by construction: a transform, plus a
   * full redraw only when the LOD band actually changes.
   */
  setCamera(camera: Camera): void {
    const nextLod = lodFor(camera.zoom);
    const bandChanged =
      nextLod.edges !== this.lod.edges ||
      nextLod.fingerprints !== this.lod.fingerprints ||
      nextLod.labels !== this.lod.labels;

    this.camera = camera;
    this.lod = nextLod;
    this.applyCamera();
    if (bandChanged) this.draw();
  }

  /** In-flight drag; redraws without touching the store (§5.1). */
  setDragging(d: { id: string; x: number; y: number } | null): void {
    this.dragging = d;
    this.draw();
  }

  /** Where an entry is right now, honouring a drag in progress. */
  positionOf(entry: { id: string; x: number; y: number }): { x: number; y: number } {
    const d = this.dragging;
    return d && d.id === entry.id ? { x: d.x, y: d.y } : { x: entry.x, y: entry.y };
  }

  /** Search filter; dims non-matches and lights the subgraph the query induces. */
  setMatched(ids: Set<string> | null, edgeIds: Set<string> | null = null): void {
    this.matched = ids;
    this.matchedEdges = edgeIds;
    this.draw();
  }

  /** True when a search is active and this entry isn't one of its hits. */
  private isDimmed(id: string): boolean {
    return this.matched !== null && !this.matched.has(id);
  }

  /** Drag-to-connect in progress; the line follows the cursor (§5.4). */
  setConnecting(c: { fromId: string; x: number; y: number } | null): void {
    this.connecting = c;
    this.draw();
  }

  /** World point where the hovered node's handle sits, or null. */
  private handleAt(id: string | null): { x: number; y: number } | null {
    if (!id) return null;
    const s = this.byId.get(id);
    if (!s) return null;
    const p = this.positionOf(s.entry);
    return { x: p.x + s.box.w / 2 + HANDLE_GAP + HANDLE_LEN / 2, y: p.y };
  }

  /** True when the pointer is over the hovered node's connect handle. */
  handleHitTest(sx: number, sy: number): string | null {
    const id = this.hoveredId;
    if (!id) return null;
    const s = this.byId.get(id);
    if (!s) return null;
    const p = this.positionOf(s.entry);
    const { x, y } = this.screenToWorld(sx, sy);
    const pad = HANDLE_HIT / this.camera.zoom;
    // One continuous band from the box edge out past the stub: any gap here
    // drops hover on the way to the handle, which makes it unreachable.
    const left = p.x + s.box.w / 2;
    const right = p.x + s.box.w / 2 + HANDLE_GAP + HANDLE_LEN + pad;
    const withinY = Math.abs(p.y - y) <= s.box.h / 2 + pad;
    return x >= left && x <= right && withinY ? id : null;
  }

  setHovered(id: string | null): void {
    if (id === this.hoveredId) return;
    this.hoveredId = id;
    this.drawOverlay();
  }

  setSelected(id: string | null): void {
    if (id === this.selectedId) return;
    this.selectedId = id;
    this.drawOverlay();
  }

  resize(width: number, height: number): void {
    this.width = width;
    this.height = height;
    if (!this.initialized) return;
    this.app.renderer.resize(width, height);
    this.applyCamera();
  }

  // -- coordinate helpers -------------------------------------------------

  screenToWorld(sx: number, sy: number): { x: number; y: number } {
    return {
      x: (sx - this.width / 2) / this.camera.zoom + this.camera.x,
      y: (sy - this.height / 2) / this.camera.zoom + this.camera.y,
    };
  }

  worldToScreen(wx: number, wy: number): { x: number; y: number } {
    return {
      x: (wx - this.camera.x) * this.camera.zoom + this.width / 2,
      y: (wy - this.camera.y) * this.camera.zoom + this.height / 2,
    };
  }

  private applyCamera(): void {
    this.world.scale.set(this.camera.zoom);
    this.world.position.set(
      this.width / 2 - this.camera.x * this.camera.zoom,
      this.height / 2 - this.camera.y * this.camera.zoom,
    );
  }

  /** Manual hit test — O(n) over a few hundred blobs, nearest wins. */
  hitTest(sx: number, sy: number): string | null {
    const { x, y } = this.screenToWorld(sx, sy);
    let best: string | null = null;
    let bestDist = Infinity;
    for (const s of this.scene.entries) {
      const p = this.positionOf(s.entry);
      const halfW = s.box.w / 2 + HIT_PAD;
      const halfH = s.box.h / 2 + HIT_PAD;
      const dx = Math.abs(p.x - x);
      const dy = Math.abs(p.y - y);
      if (dx > halfW || dy > halfH) continue;
      const d2 = dx * dx + dy * dy;
      if (d2 < bestDist) {
        bestDist = d2;
        best = s.entry.id;
      }
    }
    return best;
  }

  // -- drawing ------------------------------------------------------------

  private draw(): void {
    this.drawEdges();
    this.drawOverlay();
  }

  /** Highlight and connector share one layer, so they always redraw together. */
  private drawOverlay(): void {
    this.drawHighlight();
    this.drawConnector();
  }

  /**
   * The handle and the line being drawn from it. A stub reads as the start of
   * an edge, and it is the one small form the marks have not already claimed.
   */
  private drawConnector(): void {
    const g = this.highlightLayer;
    const from = this.connecting?.fromId ?? this.hoveredId;
    const s = from ? this.byId.get(from) : undefined;
    if (!s) return;

    const p = this.positionOf(s.entry);
    const x0 = p.x + s.box.w / 2 + HANDLE_GAP;
    g.moveTo(x0, p.y);
    g.lineTo(x0 + HANDLE_LEN, p.y);
    g.stroke({ width: 1, color: this.tokens.strokeHub, alpha: this.connecting ? 0.9 : 0.55 });

    if (!this.connecting) return;
    g.moveTo(x0 + HANDLE_LEN, p.y);
    g.lineTo(this.connecting.x, this.connecting.y);
    g.stroke({ width: 1, color: this.tokens.strokeHub, alpha: 0.7 });
  }

  /**
   * §5.4 — line weight tracks confidence: a fact renders brighter than a guess.
   * Labels are drawn by the DOM overlay, horizontal, not here.
   */
  private drawEdges(): void {
    const g = this.edgeLayer;
    g.clear();
    if (!this.lod.edges) return;

    for (const edge of this.scene.edges) {
      if (edge.status === 'dismissed') continue;
      const a = this.byId.get(edge.entryA);
      const b = this.byId.get(edge.entryB);
      if (!a || !b) continue;

      const fact = edge.status === 'accepted' || edge.status === 'manual';
      // Under a search, only edges inside the result set are drawn at all —
      // the rest would just be noise around the answer.
      const inSubgraph = this.matchedEdges?.has(edge.id) ?? false;
      if (this.matched !== null && !inSubgraph) continue;
      const pa = this.positionOf(a.entry);
      const pb = this.positionOf(b.entry);
      // Stop at each text box: the title is the node, so a line to its centre
      // would strike straight through the words.
      const sa = clipToBox(pa, pb, a.box);
      const sb = clipToBox(pb, pa, b.box);
      g.moveTo(sa.x, sa.y);
      g.lineTo(sb.x, sb.y);
      g.stroke({
        width: inSubgraph ? 1.4 : fact ? 1.15 : 0.8,
        color: inSubgraph || fact ? this.tokens.edgeFact : this.tokens.edge,
      });
    }
  }
  /** Stroke weight ramps with how much has accreted on an entry (§8 has three). */
  private strokeFor(s: SceneEntry): number {
    if (s.returns >= 3) return this.tokens.strokeHub;
    if (s.returns >= 1) return this.tokens.strokeWeighty;
    return this.tokens.strokeOrdinary;
  }
  private dashedCircle(g: Graphics, cx: number, cy: number, r: number, color: number): void {
    const arc = (Math.PI * 2) / DASH_SEGMENTS;
    for (let i = 0; i < DASH_SEGMENTS; i++) {
      const start = i * arc;
      g.arc(cx, cy, r, start, start + arc * DASH_RATIO);
      g.stroke({ width: 0.9, color, alpha: 0.85 });
    }
  }
  /** Hover and selection. Not a marker — transient state, so it stays subtle. */
  private drawHighlight(): void {
    const g = this.highlightLayer;
    g.clear();
    for (const id of [this.hoveredId, this.selectedId]) {
      if (!id) continue;
      const s = this.byId.get(id);
      if (!s) continue;
      const p = this.positionOf(s.entry);
      g.roundRect(p.x - s.box.w / 2 - 5, p.y - s.box.h / 2 - 3, s.box.w + 10, s.box.h + 6, 2);
      g.stroke({
        width: id === this.selectedId ? 1.1 : 0.8,
        color: this.tokens.strokeHub,
        alpha: id === this.selectedId ? 0.9 : 0.5,
      });
    }
  }
}
