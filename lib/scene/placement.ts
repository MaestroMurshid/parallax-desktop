/**
 * Frozen placement (§5.1): positions assigned once, never recomputed.
 * Force-directed layout would re-solve on insert and destroy spatial memory.
 *
 * Nodes are text, so everything here works on the title's box rather than a
 * circle around it. A circle that contains a wide, short title reserves the
 * empty corners too, which is what made the map look scattered while still
 * colliding horizontally.
 */

import { cosine, hash32 } from './vector';

export interface PlacedNode {
  id: string;
  x: number;
  y: number;
  halfW: number;
  halfH: number;
  isolated: boolean;
}

export interface PlacementCandidate {
  id: string;
  vec: readonly number[];
  halfW: number;
  halfH: number;
  /**
   * Ids this entry is already connected to by a named edge. A stated relation
   * is stronger evidence of relatedness than a cosine guess — §5.4's whole
   * argument — so a link outranks any similarity score. It is also the only
   * thing that gets *drawn*, and a line long enough to cross the map is a line
   * nobody can trace.
   */
  links?: readonly string[];
}

export interface PlacementOptions {
  /** Below this cosine, a neighbour isn't "strong" and doesn't pull. */
  strongThreshold: number;
  /** How many strong neighbours contribute to the target point. */
  neighbours: number;
  /** Clear space kept between boxes, in world units. */
  padX: number;
  padY: number;
  /** Step size for the outward walk. */
  step: number;
  /** Gap between the cluster's edge and the ring isolated entries sit on. */
  isolatedGap: number;
  /** Weight given to a named link. Above 1 so it outranks any cosine. */
  linkWeight: number;
  /**
   * Ceiling on the horizontal stretch of the outward walk. Uncapped it is
   * halfW/halfH, which for a one-line title is about 7 — enough to fling an
   * overflowing entry most of a screen sideways and leave the map a wide
   * stringy band. Beside-not-below is the goal; seven times is not beside.
   */
  aspectCap: number;
  /** Safety valve; the search always terminates well before this. */
  maxSteps: number;
}

export const DEFAULT_PLACEMENT: PlacementOptions = {
  strongThreshold: 0.55,
  neighbours: 3,
  padX: 30,
  padY: 26,
  step: 13,
  isolatedGap: 66,
  linkWeight: 1.25,
  aspectCap: 2.5,
  maxSteps: 4000,
};

export interface PlacementResult {
  x: number;
  y: number;
  /** ids that pulled this entry to where it landed — useful for explaining a position. */
  anchors: string[];
  /** true when nothing was similar enough and it landed in open ground (§5.1). */
  isolated: boolean;
}

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

function overlaps(
  x: number,
  y: number,
  halfW: number,
  halfH: number,
  field: readonly PlacedNode[],
  opts: PlacementOptions,
): boolean {
  for (const n of field) {
    if (
      Math.abs(n.x - x) < n.halfW + halfW + opts.padX &&
      Math.abs(n.y - y) < n.halfH + halfH + opts.padY
    ) {
      return true;
    }
  }
  return false;
}

function centroidOf(field: readonly PlacedNode[]): { x: number; y: number } {
  let cx = 0;
  let cy = 0;
  for (const n of field) {
    cx += n.x;
    cy += n.y;
  }
  return { x: cx / field.length, y: cy / field.length };
}

/**
 * Walks outward from `target` until the box fits. Two things keep this tidy:
 * the spiral starts pointing away from the crowd, so an entry that can't fit
 * spills off the near edge instead of a random compass direction; and steps
 * are stretched horizontally to match the shape of a title, so overflow lands
 * beside a neighbour rather than under it.
 */
function walkOutward(
  targetX: number,
  targetY: number,
  halfW: number,
  halfH: number,
  field: readonly PlacedNode[],
  opts: PlacementOptions,
  away: { x: number; y: number },
): { x: number; y: number } {
  if (!overlaps(targetX, targetY, halfW, halfH, field, opts)) {
    return { x: targetX, y: targetY };
  }

  const dx = targetX - away.x;
  const dy = targetY - away.y;
  const base = dx === 0 && dy === 0 ? 0 : Math.atan2(dy, dx);
  const aspect = Math.min(opts.aspectCap, Math.max(1, halfW / Math.max(halfH, 1)));

  for (let i = 1; i < opts.maxSteps; i++) {
    const r = opts.step * Math.sqrt(i);
    const a = base + i * GOLDEN_ANGLE;
    const x = targetX + Math.cos(a) * r * aspect;
    const y = targetY + Math.sin(a) * r;
    if (!overlaps(x, y, halfW, halfH, field, opts)) return { x, y };
  }
  return { x: targetX, y: targetY };
}

/**
 * §5.1: no strong neighbours → edge placement, not centroid. Centroid is the
 * tempting default but wrong — it buries an unconnected entry in the densest
 * part of the map, implying a relatedness that doesn't exist.
 *
 * They go on a ring around the cluster, stepped by golden angle over how many
 * are already out there, so a run of unrelated entries spreads evenly instead
 * of clumping or flinging the map's bounding box out to one side.
 */
function edgePlacement(
  candidate: PlacementCandidate,
  field: readonly PlacedNode[],
  opts: PlacementOptions,
): { x: number; y: number } {
  if (field.length === 0) return { x: 0, y: 0 };

  const c = centroidOf(field);

  // Measured against the connected core, not the whole field. Including the
  // entries already on this ring made each new one orbit outside the last, so
  // a corpus with several unrelated notes pushed them out in a widening spiral
  // — the cluster ended up a small knot in the middle of a mostly empty map,
  // which reads as a tangle however tidy the knot is. One ring, shared.
  const core = field.filter((n) => !n.isolated);
  const against = core.length > 0 ? core : field;

  let extentX = 0;
  let extentY = 0;
  for (const n of against) {
    extentX = Math.max(extentX, Math.abs(n.x - c.x) + n.halfW);
    extentY = Math.max(extentY, Math.abs(n.y - c.y) + n.halfH);
  }

  const rank = field.filter((n) => n.isolated).length;
  // A fixed per-id offset keeps two corpora from looking identical without
  // making the angle arbitrary.
  const jitter = (hash32(candidate.id) / 0xffffffff) * 0.4;
  const angle = rank * GOLDEN_ANGLE + jitter;

  const rx = extentX + opts.isolatedGap + candidate.halfW;
  const ry = extentY + opts.isolatedGap + candidate.halfH;

  return walkOutward(
    c.x + Math.cos(angle) * rx,
    c.y + Math.sin(angle) * ry,
    candidate.halfW,
    candidate.halfH,
    field,
    opts,
    c,
  );
}

export function placeEntry(
  candidate: PlacementCandidate,
  field: readonly PlacedNode[],
  vectors: ReadonlyMap<string, readonly number[]>,
  opts: PlacementOptions = DEFAULT_PLACEMENT,
): PlacementResult {
  if (field.length === 0) {
    return { x: 0, y: 0, anchors: [], isolated: true };
  }

  const linked = candidate.links?.length ? new Set(candidate.links) : null;

  const scored: Array<{ node: PlacedNode; sim: number }> = [];
  for (const node of field) {
    if (linked?.has(node.id)) {
      scored.push({ node, sim: opts.linkWeight });
      continue;
    }
    const v = vectors.get(node.id);
    if (!v) continue;
    const sim = cosine(candidate.vec, v);
    if (sim >= opts.strongThreshold) scored.push({ node, sim });
  }

  if (scored.length === 0) {
    const { x, y } = edgePlacement(candidate, field, opts);
    return { x, y, anchors: [], isolated: true };
  }

  scored.sort((a, b) => b.sim - a.sim);
  const top = scored.slice(0, opts.neighbours);

  // Similarity-weighted centroid of the strong neighbours. Weighting matters:
  // an entry that is 0.9 to one note and 0.56 to another belongs near the first.
  let wx = 0;
  let wy = 0;
  let wsum = 0;
  for (const { node, sim } of top) {
    const w = sim - opts.strongThreshold + 0.01;
    wx += node.x * w;
    wy += node.y * w;
    wsum += w;
  }

  const { x, y } = walkOutward(
    wx / wsum,
    wy / wsum,
    candidate.halfW,
    candidate.halfH,
    field,
    opts,
    centroidOf(field),
  );

  return { x, y, anchors: top.map((t) => t.node.id), isolated: false };
}

/**
 * Replays placement over an ordered corpus, so the seeded fixture's coordinates
 * come from the real algorithm (not hand-authored) and exercise the same code
 * path a live insert takes. Order matters — it's what makes position encode time.
 */
export function placeCorpus(
  candidates: readonly PlacementCandidate[],
  opts: PlacementOptions = DEFAULT_PLACEMENT,
): Map<string, PlacementResult> {
  const field: PlacedNode[] = [];
  const vectors = new Map<string, readonly number[]>();
  const out = new Map<string, PlacementResult>();

  for (const c of candidates) {
    const result = placeEntry(c, field, vectors, opts);
    out.set(c.id, result);
    field.push({
      id: c.id,
      x: result.x,
      y: result.y,
      halfW: c.halfW,
      halfH: c.halfH,
      isolated: result.isolated,
    });
    vectors.set(c.id, c.vec);
  }
  return out;
}
