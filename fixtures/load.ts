/**
 * Turns the authored seed corpus into live domain objects. Positions and
 * fingerprints are derived, not authored — real §5.1 placement algorithm, so
 * the fixture exercises the same code path a live insert takes.
 */

import type { ActionItem, Edge, Entry, Question, Span } from '@/lib/types';
import { FINGERPRINT_MIN_BARS, FINGERPRINT_MAX_BARS } from '@/lib/scene/blob';
import { titleSizeForDuration, wrapTitle } from '@/lib/scene/lexicon';
import { placeCorpus, type PlacementCandidate } from '@/lib/scene/placement';
import { relaxLayout } from '@/lib/scene/relax';
import { detectUnfinished } from '@/lib/scene/markers';
import { mayProbeAutomatically } from '@/lib/scene/classification';
import { hash32, mockVector, rng } from '@/lib/scene/vector';
import type { SeedCorpus, SeedEntry } from './types';
import raw from './corpus.json';

const seed = raw as unknown as SeedCorpus;

/**
 * Plausible speech amplitude: an onset, a body with breath gaps, a tail.
 * §5.2 wants 7–9 bars — fewer looks sparse, more makes recordings look alike.
 */
function makeFingerprint(id: string, durationMs: number): number[] {
  const r = rng(hash32(id));
  const bars = FINGERPRINT_MIN_BARS + Math.floor(r() * (FINGERPRINT_MAX_BARS - FINGERPRINT_MIN_BARS + 1));
  // Longer recordings downsample more speech into each bar, so they read
  // fuller; short ones keep their gaps (§5.2).
  const density = Math.min(1, durationMs / 240_000);
  const out: number[] = [];
  for (let i = 0; i < bars; i++) {
    const envelope = Math.sin((i / (bars - 1)) * Math.PI) * 0.35 + 0.5;
    const jitter = r();
    const gap = jitter < 0.12 * (1 - density) ? 0.18 : 1;
    out.push(Math.min(1, Math.max(0.12, envelope * (0.55 + jitter * 0.6) * gap)));
  }
  return out;
}

function findSpan(transcript: string, quote: string, attributed: boolean): Span | null {
  const start = transcript.indexOf(quote);
  if (start < 0) return null;
  return { start, end: start + quote.length, attributed };
}

/** Placement works on the title's box, so hand it the box. */
function placementBox(e: SeedEntry): { halfW: number; halfH: number } {
  const size = titleSizeForDuration(e.durationMs);
  const lines = wrapTitle(e.title);
  const longest = lines.reduce((m, l) => Math.max(m, l.length), 0);
  return {
    halfW: (longest * size * 0.55) / 2,
    halfH: (lines.length * size * 1.13) / 2,
  };
}

function toEntry(s: SeedEntry, x: number, y: number): Entry {
  const spans: Span[] = [];
  for (const q of s.attributedQuotes) {
    const span = findSpan(s.transcript, q, true);
    if (span) spans.push(span);
  }

  const actionItems: ActionItem[] = [];
  s.actionItems.forEach((text, i) => {
    const span = findSpan(s.transcript, text, false);
    if (span) {
      actionItems.push({ id: `${s.id}-task-${i}`, entryId: s.id, span, text, done: false });
    }
  });

  return {
    id: s.id,
    audioPath: s.audio ? `sample://${s.id}.wav` : null,
    transcript: s.transcript,
    createdAt: s.createdAt,
    x,
    y,
    parentEdge: s.parentEdge,
    role: s.role,
    register: s.register,
    typeId: s.typeId,
    resolved: s.resolved,
    resolutionText: s.resolutionText,
    title: s.title,
    // Never generated for live entries (§1.1) — belt and braces over the fixture.
    summary: s.register === 'live' ? null : s.summary,
    durationMs: s.durationMs,
    fingerprint: s.audio ? makeFingerprint(s.id, s.durationMs) : [],
    unfinished: detectUnfinished(s.transcript),
    localOnly: s.localOnly,
    spans,
    actionItems,
    isSample: true,
  };
}

export interface LoadedCorpus {
  entries: Entry[];
  edges: Edge[];
  questions: Question[];
  actionItems: ActionItem[];
}

export function loadSeedCorpus(): LoadedCorpus {
  const chronological = [...seed.entries].sort((a, b) => a.createdAt.localeCompare(b.createdAt));

  // Only root entries occupy the canvas. A child is a layer on its parent — it
  // thickens the ring rather than spawning its own blob (§6.2, §11).
  const roots = chronological.filter((e) => e.parentEdge === null);

  // What the map will actually draw. Feeding it to placement is what keeps a
  // drawn line short enough to follow, rather than a guess about topic putting
  // two linked entries on opposite sides of the field.
  const links = new Map<string, string[]>();
  const link = (from: string, to: string) => {
    const existing = links.get(from);
    if (existing) existing.push(to);
    else links.set(from, [to]);
  };
  for (const g of seed.edges) {
    link(g.a, g.b);
    link(g.b, g.a);
  }

  const candidates: PlacementCandidate[] = roots.map((e) => ({
    id: e.id,
    vec: mockVector(e.topicCluster, e.id),
    links: links.get(e.id) ?? [],
    ...placementBox(e),
  }));

  const placed = placeCorpus(candidates);

  // Then settle it, the same pass the tidy control runs.
  //
  // This is not a shortcut around §5.1. Frozen placement is a promise about
  // what the app does *to entries you already made* — it never re-solves behind
  // your back. The seed corpus is a year of entries that arrived one at a time,
  // and someone with a year of entries would have tidied at some point; opening
  // it mid-tangle demonstrates the algorithm's first draft rather than the state
  // the app is actually used in. Still derived, still deterministic, and the
  // live insert path is untouched.
  const settled = relaxLayout(
    candidates.map((c) => {
      const p = placed.get(c.id);
      return { id: c.id, x: p?.x ?? 0, y: p?.y ?? 0, halfW: c.halfW, halfH: c.halfH };
    }),
    links,
  );
  const at = (id: string) => settled.get(id) ?? placed.get(id);

  const entries: Entry[] = [];
  for (const s of chronological) {
    if (s.parentEdge === null) {
      const p = at(s.id);
      entries.push(toEntry(s, p?.x ?? 0, p?.y ?? 0));
    } else {
      // Children inherit the parent's position; they are never drawn separately.
      const parent = at(s.parentEdge);
      entries.push(toEntry(s, parent?.x ?? 0, parent?.y ?? 0));
    }
  }

  const byId = new Map(entries.map((e) => [e.id, e]));

  const edges: Edge[] = seed.edges
    .filter((e) => byId.has(e.a) && byId.has(e.b))
    .map((e, i) => ({
      id: `edge-${i}`,
      entryA: e.a,
      entryB: e.b,
      relation: e.relation,
      question: e.question,
      status: e.status,
      createdAt: byId.get(e.b)!.createdAt,
    }));

  const questions: Question[] = seed.questions
    .filter((q) => byId.has(q.entryId))
    .map((q, i) => {
      const entry = byId.get(q.entryId)!;
      return {
        id: `question-${i}`,
        entryId: q.entryId,
        text: q.text,
        span: q.spanQuote ? findSpan(entry.transcript, q.spanQuote, false) : null,
        answered: q.answered,
        providerName: q.providerName,
        createdAt: entry.createdAt,
      };
    })
    // §3.2 suppression is structural, not advisory: the three facets decide
    // what may carry an automatic question, whatever the fixture says.
    .filter((q) => mayProbeAutomatically(byId.get(q.entryId)!));

  const actionItems = entries.flatMap((e) => e.actionItems);

  return { entries, edges, questions, actionItems };
}
