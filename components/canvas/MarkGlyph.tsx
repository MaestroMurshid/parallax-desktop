'use client';

import type { Mark, SlotId } from '@/lib/scene/classification';
import { MARK_BOX, validateMark } from '@/lib/scene/mark-validator';

/** Validation touches a canvas, so results are cached per character. */
const sizeCache = new Map<string, number | null>();

function charFontSize(char: string): number | null {
  if (!sizeCache.has(char)) {
    const r = validateMark(char);
    sizeCache.set(char, r.ok ? r.fontSize : null);
  }
  return sizeCache.get(char) ?? null;
}

const B = MARK_BOX;

function glyph(id: SlotId) {
  switch (id) {
    // An assertion stands up.
    case 'position':
      return <rect x={B / 2 - 0.8} y={0.5} width={1.6} height={8} fill="var(--title)" />;
    // A fact lies flat — the same stroke, laid down.
    case 'evidence':
      return <rect x={B / 2 - 4} y={3.7} width={8} height={1.6} fill="var(--title)" />;
    case 'note':
      return (
        <rect
          x={B / 2 - 2}
          y={2.5}
          width={4}
          height={4}
          fill="none"
          stroke="var(--meta)"
          strokeWidth={0.9}
        />
      );
  }
}

/** `size` may be any CSS length so a node can scale its mark with the camera. */
export default function MarkGlyph({
  mark,
  size = MARK_BOX,
}: {
  mark: Mark | null;
  size?: number | string;
}) {
  const box = typeof size === 'number' ? `${size}px` : size;
  if (!mark) return <span style={{ display: 'inline-block', width: box }} aria-hidden />;

  if (mark.kind === 'char') {
    const fs = charFontSize(mark.char);
    if (fs === null) return <span style={{ display: 'inline-block', width: box }} aria-hidden />;
    return (
      <span
        aria-hidden
        style={{
          display: 'inline-flex',
          justifyContent: 'center',
          alignItems: 'center',
          width: box,
          height: box,
          fontFamily: 'var(--font-mono)',
          fontSize: `calc(${box} * ${fs / B})`,
          lineHeight: 1,
          color: 'var(--meta)',
        }}
      >
        {mark.char}
      </span>
    );
  }

  return (
    <svg width={box} height={box} viewBox={`0 0 ${B} ${B}`} aria-hidden focusable="false">
      {glyph(mark.id)}
    </svg>
  );
}
