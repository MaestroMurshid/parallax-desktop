/**
 * Reads the §8 palette out of CSS custom properties so Pixi and DOM chrome
 * can't drift apart. Resolved once at init and cached — no theming, the
 * palette is static at runtime (§8).
 */

export interface Tokens {
  surface: number;
  blobFill: number;
  blobFillFelt: number;
  strokeOrdinary: number;
  strokeWeighty: number;
  strokeHub: number;
  edge: number;
  edgeFact: number;
  title: string;
  edgeLabel: string;
  dimmedFg: number;
  dimmedBg: number;
  ring: [number, number, number];
  accent: number;
}

function readVar(style: CSSStyleDeclaration, name: string): string {
  const v = style.getPropertyValue(name).trim();
  if (!v) throw new Error(`Missing CSS token: ${name}`);
  return v;
}

/** "#1c2224" → 0x1c2224, the form Pixi wants. */
export function hexToNumber(hex: string): number {
  const h = hex.replace('#', '');
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
  return parseInt(full, 16);
}

export function readTokens(el: HTMLElement = document.documentElement): Tokens {
  const s = getComputedStyle(el);
  const n = (name: string) => hexToNumber(readVar(s, name));
  return {
    surface: n('--surface'),
    blobFill: n('--blob-fill'),
    blobFillFelt: n('--blob-fill-felt'),
    strokeOrdinary: n('--stroke-ordinary'),
    strokeWeighty: n('--stroke-weighty'),
    strokeHub: n('--stroke-hub'),
    edge: n('--edge'),
    edgeFact: n('--edge-fact'),
    title: readVar(s, '--title'),
    edgeLabel: readVar(s, '--edge-label'),
    dimmedFg: n('--dimmed-fg'),
    dimmedBg: n('--dimmed-bg'),
    ring: [n('--ring-1'), n('--ring-2'), n('--ring-3')],
    accent: n('--accent'),
  };
}
