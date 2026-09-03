/**
 * Seed corpus types (§13). Positions and fingerprints are NOT authored — both
 * are derived at load time (real §5.1 placement, deterministic per-id). Spans
 * anchor by quoted substring, not offset, so transcripts stay editable.
 */

import type { Relation, RenderedType } from '@/lib/types';

export interface SeedEntry {
  id: string;
  /** ISO date. The corpus spans roughly a year; order is meaningful (§5.1). */
  createdAt: string;
  /** Verbatim speech. This is the record and is never rewritten (§2). */
  transcript: string;
  /** 3–4 words, drawn from the user's own phrasing wherever possible (§5.2). */
  title: string;
  /** null for felt entries — flattening those is worse than useless (§1.1). */
  summary: string | null;
  type: RenderedType;
  /** Finer classification, stored not rendered (§3.6). */
  storedType: string;
  durationMs: number;
  /** false => typed entry: no fingerprint, a free visual distinction (§4). */
  audio: boolean;
  /** Drives topic_vec in the mock. Entries in one cluster sit near each other. */
  topicCluster: string;
  /** Drives move_vec — what the entry *does*, independent of subject (§7.1). */
  moveCluster: string;
  /** Set => an answer; thickens the parent's ring rather than getting a blob (§6.2). */
  parentEdge: string | null;
  resolved: boolean;
  /** Stating what the resolution *is* is the point; a bare flag gives nothing (§6.3). */
  resolutionText: string | null;
  localOnly: boolean;
  /** Quoted material, excluded from the move vector (§7.3). Substrings of transcript. */
  attributedQuotes: string[];
  /** Exact substrings of transcript. Ticking is span state, not a text edit (§1.2). */
  actionItems: string[];
  /** Slot reserved for one of the user's real entries; transcript is a stand-in. */
  placeholderForReal?: string;
}

export interface SeedEdge {
  a: string;
  b: string;
  /** §5.4 — if the best you can say is "related", draw nothing. */
  relation: Relation;
  status: 'proposed' | 'accepted' | 'manual';
  question: string | null;
}

export interface SeedQuestion {
  entryId: string;
  text: string;
  /** Substring of the entry transcript. Every analytical claim quotes a span (§3.4). */
  spanQuote: string | null;
  answered: boolean;
  providerName: string;
}

export interface SeedCorpus {
  entries: SeedEntry[];
  edges: SeedEdge[];
  questions: SeedQuestion[];
}
