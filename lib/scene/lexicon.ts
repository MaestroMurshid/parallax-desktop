/**
 * Lexicon geometry. The title is the node — there is no blob — so duration
 * drives type size and the node's hit box is the text box.
 */

import { hash32, rng } from './vector';
import type { Entry } from '@/lib/types';

const REF_SECONDS = 60;
const REF_SIZE = 13;
export const MIN_SIZE = 10.5;
export const MAX_SIZE = 19;

/** Square-root, as blob radius was: a six-minute ramble reads large without
 *  swallowing the map. */
export function titleSizeForDuration(durationMs: number): number {
  const seconds = Math.max(durationMs, 0) / 1000;
  const s = REF_SIZE * Math.sqrt(seconds / REF_SECONDS);
  return Math.min(MAX_SIZE, Math.max(MIN_SIZE, s));
}

export function titleSizeFor(entry: Entry): number {
  return titleSizeForDuration(entry.durationMs);
}

const WRAP_CHARS = 14;
const LINE_RATIO = 1.13;
const CHAR_RATIO = { serif: 0.46, mono: 0.6 } as const;

export function wrapTitle(title: string, limit = WRAP_CHARS): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of title.split(/\s+/)) {
    if (!line) line = word;
    else if (line.length + 1 + word.length <= limit) line += ` ${word}`;
    else {
      lines.push(line);
      line = word;
    }
  }
  if (line) lines.push(line);
  return lines.length ? lines : [title];
}

export interface TitleBox {
  w: number;
  h: number;
  lines: string[];
}

/** World-space box, used for hit testing and for framing the corpus. */
export function titleBox(entry: Entry, family: 'serif' | 'mono' = 'serif'): TitleBox {
  const size = titleSizeFor(entry);
  const lines = wrapTitle(entry.title);
  const longest = lines.reduce((m, l) => Math.max(m, l.length), 0);
  return {
    w: longest * size * CHAR_RATIO[family],
    h: lines.length * size * LINE_RATIO,
    lines,
  };
}

export interface Bar {
  x: number;
  w: number;
  h: number;
}

/**
 * The audio signature under a title. Deterministic per id — §5.1 freezes
 * appearance with position, so it can't reshape between sessions.
 */
/** §5.2 — 7–9 samples, downsampled from real amplitude at capture. */
export const FINGERPRINT_MIN_BARS = 7;
export const FINGERPRINT_MAX_BARS = 9;

const BAR_W = 1.3;
const BAR_GAP = 2.2;
const BAR_MIN_H = 1.3;
const BAR_MAX_H = 4.3;

/**
 * The signature under a title, drawn from what was actually said. A typed
 * entry has no fingerprint and so gets no bars, which is the distinction §4
 * wanted and costs nothing to draw.
 */
export function signatureBars(fingerprint: readonly number[], width: number): Bar[] {
  if (fingerprint.length === 0) return [];

  const fit = Math.max(1, Math.floor(width / (BAR_W + BAR_GAP)));
  const count = Math.min(fingerprint.length, fit);
  const out: Bar[] = [];
  for (let i = 0; i < count; i++) {
    // Sample across the whole fingerprint rather than truncating it, so a
    // narrow node still shows the shape of the whole recording.
    const v = fingerprint[Math.round((i * (fingerprint.length - 1)) / Math.max(1, count - 1))] ?? 0;
    out.push({
      x: i * (BAR_W + BAR_GAP),
      w: BAR_W,
      h: BAR_MIN_H + Math.max(0, Math.min(1, v)) * (BAR_MAX_H - BAR_MIN_H),
    });
  }
  return out;
}

export const SIGNATURE_HEIGHT = 4.4;
