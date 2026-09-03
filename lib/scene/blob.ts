/**
 * Blob geometry (§5.2). Three visual channels and no more (§5.3): size, edge
 * treatment, fingerprint. Action items (§1.2) and emotion (§11) stay off canvas.
 */

import { hash32, rng } from './vector';
import type { Entry } from '@/lib/types';

/** Duration → radius (§5.2). Square-root so area (not radius) tracks duration —
 *  a six-minute ramble reads as large without swallowing the map. */
const REF_SECONDS = 60;
const REF_RADIUS = 24;
export const MIN_RADIUS = 12;
export const MAX_RADIUS = 52;

export function radiusForDuration(durationMs: number): number {
  const seconds = Math.max(durationMs, 0) / 1000;
  const r = REF_RADIUS * Math.sqrt(seconds / REF_SECONDS);
  return Math.min(MAX_RADIUS, Math.max(MIN_RADIUS, r));
}

export function radiusForEntry(entry: Entry): number {
  return radiusForDuration(entry.durationMs);
}

/**
 * Irregular outline for `rant` (§5.2). Means unresolved, not angry — §11
 * rejects emotion encoding. Deterministic per id: §5.1 freezes appearance
 * with position, so it can't reshape between sessions.
 */
export function irregularOutline(id: string, radius: number, points = 24, amount = 0.09): Array<[number, number]> {
  const r = rng(hash32(id));
  // Three low-frequency harmonics with fixed phases: smooth, closed, no cusps.
  const h = [
    { amp: r(), phase: r() * Math.PI * 2, freq: 2 },
    { amp: r() * 0.6, phase: r() * Math.PI * 2, freq: 3 },
    { amp: r() * 0.35, phase: r() * Math.PI * 2, freq: 5 },
  ];
  const out: Array<[number, number]> = [];
  for (let i = 0; i < points; i++) {
    const a = (i / points) * Math.PI * 2;
    let m = 0;
    for (const { amp, phase, freq } of h) m += amp * Math.sin(a * freq + phase);
    const rr = radius * (1 + amount * (m / 1.95));
    out.push([Math.cos(a) * rr, Math.sin(a) * rr]);
  }
  return out;
}

/** Fingerprint bars (§5.2): 7–9 bars downsampled from amplitude, so every
 *  recording looks different — fewer and they'd start looking alike. */
export const FINGERPRINT_MIN_BARS = 7;
export const FINGERPRINT_MAX_BARS = 9;

export interface FingerprintBar {
  x: number;
  height: number;
  width: number;
}

export function fingerprintBars(fingerprint: readonly number[], radius: number): FingerprintBar[] {
  if (fingerprint.length === 0) return []; // typed entry — no fingerprint (§4)
  const n = Math.min(Math.max(fingerprint.length, FINGERPRINT_MIN_BARS), FINGERPRINT_MAX_BARS);
  // Keep the bars inside the blob with room to spare; they are texture, not fill.
  const span = radius * 1.05;
  const width = Math.max(1.5, (span / n) * 0.5);
  const gap = (span - width * n) / Math.max(n - 1, 1);
  const maxH = radius * 0.92;
  const bars: FingerprintBar[] = [];
  for (let i = 0; i < n; i++) {
    const level = Math.min(1, Math.max(0, fingerprint[i] ?? 0));
    bars.push({
      x: -span / 2 + i * (width + gap) + width / 2,
      height: Math.max(width, level * maxH),
      width,
    });
  }
  return bars;
}

/**
 * Return rings (§5.3): ring count = number of returns, surfacing info (§6.2)
 * otherwise invisible. Clashes with playing-audio pulse rings — `asArc` renders
 * partial arcs so both can share the canvas.
 */
export const MAX_RENDERED_RINGS = 4;
const RING_GAP = 4.5;

export interface RingSpec {
  radius: number;
  /** Which --ring-N token to use; outer rings are darkest (§8). */
  tone: 1 | 2 | 3;
  /** Solid outer ring = resolved, user-declared (§5.3, §6.3). */
  solid: boolean;
  asArc: boolean;
}

export function ringSpecs(returns: number, resolved: boolean, radius: number, audioPlaying = false): RingSpec[] {
  const count = Math.min(returns, MAX_RENDERED_RINGS);
  const specs: RingSpec[] = [];
  for (let i = 0; i < count; i++) {
    // i = 0 is the innermost ring; the outermost is the darkest.
    const fromOuter = count - 1 - i;
    specs.push({
      radius: radius + RING_GAP * (i + 1),
      tone: (Math.min(fromOuter, 2) + 1) as 1 | 2 | 3,
      solid: false,
      asArc: audioPlaying,
    });
  }
  if (resolved) {
    specs.push({
      radius: radius + RING_GAP * (count + 1),
      tone: 3,
      solid: true,
      asArc: audioPlaying,
    });
  }
  return specs;
}

/** Where the unanswered-question dot sits — the only saturated element (§5.3). */
export function questionDotOffset(radius: number): { x: number; y: number; r: number } {
  const a = -Math.PI / 4;
  return { x: Math.cos(a) * radius, y: Math.sin(a) * radius, r: 2.6 };
}
