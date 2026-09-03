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
export function signatureBars(id: string, width: number): Bar[] {
  const barW = 1.3;
  const gap = 2.2;
  const count = Math.max(5, Math.min(10, Math.round(width / (barW + gap))));
  const r = rng(hash32(id));
  const out: Bar[] = [];
  for (let i = 0; i < count; i++) {
    out.push({ x: i * (barW + gap), w: barW, h: 1.3 + r() * 3 });
  }
  return out;
}

export const SIGNATURE_HEIGHT = 4.4;
