/**
 * Markers (§5.3). Allocation principle: type is a guess, so it stays quiet;
 * an unanswered question is certain and actionable, so it gets the app's one
 * saturated element.
 */

import type { Entry } from '@/lib/types';

export interface EntryMarkers {
  /** Left unfinished — dashed stroke. From the user's own hedges. */
  unfinished: boolean;
  /** Concentric rings; one per return. */
  returns: number;
  /** Single solid outer ring. User-declared only (§6.3). */
  resolved: boolean;
  /** Orange dot — THE one colour. */
  unansweredQuestion: boolean;
  /** Dimmed fill, no edges. */
  isolated: boolean;
}

/**
 * §5.3: unfinished is detected by **regex, not a model**. The user's own hedges
 * are a reliable signal and a model here would produce confident false
 * positives on exactly the entries where being wrong is least recoverable.
 */
const HEDGES = [
  /\bidk\b/i,
  /\bi don'?t know\b/i,
  /\bi'?m not sure\b/i,
  /\bnot sure what\b/i,
  /\bi don'?t wanna say\b/i,
  /\bi don'?t want to say\b/i,
  /\bcan'?t quite\b/i,
  /\bsomething like that\b/i,
  /\bor whatever\b/i,
  /\bhaven'?t worked out\b/i,
  /\bstill figuring\b/i,
  /\bmaybe\?/i,
];

export function detectUnfinished(transcript: string): boolean {
  return HEDGES.some((re) => re.test(transcript));
}

export interface MarkerContext {
  returns: number;
  hasUnansweredQuestion: boolean;
  hasEdges: boolean;
}

export function markersFor(entry: Entry, ctx: MarkerContext): EntryMarkers {
  return {
    unfinished: entry.unfinished,
    returns: ctx.returns,
    resolved: entry.resolved,
    unansweredQuestion: ctx.hasUnansweredQuestion,
    isolated: !ctx.hasEdges,
  };
}

/**
 * Edge treatment by type (§5.2) — reads as texture at canvas scale, not text.
 * §5.3 known issue: the soft felt blob gets a faint stroke below the
 * fingerprint threshold so it doesn't vanish at low zoom.
 */
export type EdgeTreatment = 'crisp' | 'irregular' | 'soft' | 'plain';

export function edgeTreatment(entry: Entry): EdgeTreatment {
  switch (entry.type) {
    case 'claim':
      return 'crisp';
    case 'rant':
      return 'irregular';
    case 'felt':
      return 'soft';
    case 'inert':
      return 'plain';
  }
}
