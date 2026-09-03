/**
 * Similarity primitives (§9.0): brute-force cosine is the whole retrieval
 * layer at this scale — no vector database needed. Mock corpus uses 24 dims,
 * not 384; nothing downstream depends on the dimension.
 */

export function dot(a: readonly number[], b: readonly number[]): number {
  const n = Math.min(a.length, b.length);
  let sum = 0;
  for (let i = 0; i < n; i++) sum += a[i]! * b[i]!;
  return sum;
}

export function norm(a: readonly number[]): number {
  return Math.sqrt(dot(a, a));
}

export function cosine(a: readonly number[], b: readonly number[]): number {
  const d = norm(a) * norm(b);
  return d === 0 ? 0 : dot(a, b) / d;
}

export function normalize(a: readonly number[]): number[] {
  const n = norm(a);
  return n === 0 ? [...a] : a.map((v) => v / n);
}

/**
 * Deterministic 32-bit hash for a stable pseudo-random value keyed to an id —
 * placement fallback angles, blob irregularity. Must be deterministic since
 * position is frozen forever (§5.1).
 */
export function hash32(s: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h >>> 0;
}

/** Seeded PRNG (mulberry32). Deterministic given a seed. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Deterministic unit vector near a named cluster centroid. Fixture-only — lets
 * the seeded corpus drive the real placement algorithm via real cosine
 * similarity, not hand-authored coordinates. Replaced by fastembed on Rust.
 */
export function mockVector(cluster: string, id: string, dims = 24, jitter = 0.35): number[] {
  const centroid = rng(hash32(cluster));
  const local = rng(hash32(id));
  const v: number[] = [];
  for (let i = 0; i < dims; i++) {
    v.push(centroid() * 2 - 1 + (local() * 2 - 1) * jitter);
  }
  return normalize(v);
}
