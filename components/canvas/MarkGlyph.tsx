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
    case 'claim':
      return <rect x={B / 2 - 0.8} y={0.5} width={1.6} height={8} fill="var(--title)" />;
    case 'rant':
      return (
        <path
          d={`M${B / 2 - 1.5},0.5 q3,2 0,4 q-3,2 0,4`}
          fill="none"
          stroke="var(--title)"
          strokeWidth={1}
        />
      );
    case 'felt':
      return (
        <rect x={B / 2 - 3} y={3.1} width={6} height={2.8} rx={1.4} fill="var(--meta)" opacity={0.5} />
      );
    case 'inert':
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
