/**
 * Validates a pasted mark glyph. The rail normalises every mark into a fixed
 * box scaled by ink height, so an arbitrary character is safe once it clears
 * four failures: colour emoji, a missing glyph, too much ink, and the disc
 * shape reserved for the unanswered question.
 */

export const MARK_BOX = 9;
export const TARGET_INK_HEIGHT = 7;
const FALLBACK_FONT = `ui-monospace, SFMono-Regular, Consolas, monospace`;

/** Must measure in the font the app actually renders in, or tofu detection and
 *  the coverage numbers are about a font nobody sees. */
function markFont(): string {
  if (typeof document === 'undefined') return FALLBACK_FONT;
  const v = getComputedStyle(document.documentElement).getPropertyValue('--font-mono').trim();
  return v || FALLBACK_FONT;
}
/** Built-in marks measure 0.10-0.16 coverage; this allows roughly double. */
const MAX_COVERAGE = 0.3;
/** A tall narrow solid glyph normalises small enough to slip the coverage cap,
 *  so solidity is checked on its own. Hairlines like | stay under the aspect. */
const SOLID_FILL = 0.9;
const SOLID_ASPECT = 0.25;
const PROBE = 64;
const NOTDEF = '\u{10FFFF}';
const CAP_REFERENCE = 'H';
const IS_TEXT = /^[\p{L}\p{N}]$/u;

export type RejectReason =
  | 'empty'
  | 'emoji'
  | 'no-glyph'
  | 'too-heavy'
  | 'reserved-shape'
  | 'no-canvas';

export interface MarkOk {
  ok: true;
  char: string;
  /** Render at this font-size to land on TARGET_INK_HEIGHT of ink. */
  fontSize: number;
  coverage: number;
}

export type MarkResult = MarkOk | { ok: false; reason: RejectReason };

export const REJECT_MESSAGE: Record<RejectReason, string> = {
  empty: 'Nothing to show — this character has no width.',
  emoji: 'Emoji carry their own colour, and the map has exactly one.',
  'no-glyph': 'This font has no glyph for that — it would render as a box.',
  'too-heavy': 'Too much ink. Weight reads as importance, which duration already means.',
  'reserved-shape': 'A filled circle is the unanswered question, and only that.',
  'no-canvas': 'Cannot measure this here.',
};

/** First grapheme cluster only — ZWJ sequences and flags collapse to one. */
export function firstGrapheme(input: string): string {
  const s = input.trim();
  if (!s) return '';
  const Seg = (Intl as { Segmenter?: typeof Intl.Segmenter }).Segmenter;
  if (!Seg) return [...s][0] ?? '';
  for (const g of new Seg(undefined, { granularity: 'grapheme' }).segment(s)) return g.segment;
  return [...s][0] ?? '';
}

interface Ink {
  pixels: number;
  w: number;
  h: number;
  fillRatio: number;
  grid: number[];
  cornerRatio: number;
}

function measure(ctx: CanvasRenderingContext2D, char: string, font: string): Ink | null {
  ctx.clearRect(0, 0, PROBE * 2, PROBE * 2);
  ctx.fillStyle = '#000';
  ctx.font = `${PROBE}px ${font}`;
  ctx.textBaseline = 'middle';
  ctx.fillText(char, PROBE * 0.5, PROBE);

  const { data } = ctx.getImageData(0, 0, PROBE * 2, PROBE * 2);
  let minX = Infinity, minY = Infinity, maxX = -1, maxY = -1, pixels = 0;
  for (let y = 0; y < PROBE * 2; y++) {
    for (let x = 0; x < PROBE * 2; x++) {
      if (data[(y * PROBE * 2 + x) * 4 + 3]! < 40) continue;
      pixels++;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }
  if (pixels === 0) return null;

  const w = maxX - minX + 1;
  const h = maxY - minY + 1;

  // 8x8 ink signature, used to recognise the notdef box.
  const grid: number[] = [];
  for (let gy = 0; gy < 8; gy++) {
    for (let gx = 0; gx < 8; gx++) {
      let hit = 0, n = 0;
      const x0 = minX + Math.floor((gx * w) / 8), x1 = minX + Math.floor(((gx + 1) * w) / 8);
      const y0 = minY + Math.floor((gy * h) / 8), y1 = minY + Math.floor(((gy + 1) * h) / 8);
      for (let y = y0; y < Math.max(y1, y0 + 1); y++) {
        for (let x = x0; x < Math.max(x1, x0 + 1); x++) {
          n++;
          if (data[(y * PROBE * 2 + x) * 4 + 3]! >= 40) hit++;
        }
      }
      grid.push(n ? hit / n : 0);
    }
  }

  // Corners empty + a solid middle is what separates a disc from a block.
  const corners = [grid[0]!, grid[7]!, grid[56]!, grid[63]!];
  return {
    pixels,
    w,
    h,
    fillRatio: pixels / (w * h),
    grid,
    cornerRatio: corners.reduce((a, b) => a + b, 0) / 4,
  };
}

const gridsMatch = (a: number[], b: number[]) =>
  a.every((v, i) => Math.abs(v - (b[i] ?? 0)) < 0.12);

export function validateMark(input: string): MarkResult {
  const char = firstGrapheme(input);
  if (!char) return { ok: false, reason: 'empty' };
  if (/\p{Extended_Pictographic}/u.test(char)) return { ok: false, reason: 'emoji' };

  if (typeof document === 'undefined') return { ok: false, reason: 'no-canvas' };
  const canvas = document.createElement('canvas');
  canvas.width = PROBE * 2;
  canvas.height = PROBE * 2;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) return { ok: false, reason: 'no-canvas' };

  const font = markFont();
  const ink = measure(ctx, char, font);
  if (!ink) return { ok: false, reason: 'empty' };

  const tofu = measure(ctx, NOTDEF, font);
  if (tofu && gridsMatch(ink.grid, tofu.grid)) return { ok: false, reason: 'no-glyph' };

  // Letters scale off cap height, not their own ink: an x-height glyph scaled
  // to full ink height renders larger than a capital and turns heavy.
  const capRef = IS_TEXT.test(char) ? measure(ctx, CAP_REFERENCE, font) : null;
  const basis = capRef ? capRef.h : ink.h;
  const scale = TARGET_INK_HEIGHT / basis;
  const coverage = (ink.pixels * scale * scale) / (MARK_BOX * MARK_BOX);
  const aspect = ink.w / ink.h;

  const round = aspect > 0.85 && aspect < 1.18 && ink.fillRatio > 0.55 && ink.cornerRatio < 0.2;
  if (round) return { ok: false, reason: 'reserved-shape' };
  const solid = ink.fillRatio > SOLID_FILL && aspect > SOLID_ASPECT;
  if (solid || coverage > MAX_COVERAGE) return { ok: false, reason: 'too-heavy' };

  return { ok: true, char, fontSize: TARGET_INK_HEIGHT * (PROBE / basis), coverage };
}
