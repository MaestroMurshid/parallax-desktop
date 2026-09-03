/**
 * Domain types — mirrors the §7 schema (titles §5.2, summaries §7.1, action items §1.2).
 * Also the wire types for the bridge; the Rust side serialises to this exact shape.
 */

// ---------------------------------------------------------------------------
// Classification — §3.6, two axes that scale differently.
// ---------------------------------------------------------------------------

/**
 * Axis 1, rendered. A blob carries roughly three visual encodings before it
 * becomes noise (§5.3), so the canvas only ever sees these four.
 */
export type RenderedType = 'claim' | 'rant' | 'felt' | 'inert';

/**
 * Axis 1, stored. Deliberately a string, not a union — user-defined types (§3.6)
 * live here too. Costs nothing on canvas since we render the collapse, not this.
 */
export type StoredType = string;

/**
 * Probe risk tiers (§3.6). Modes may proliferate within a tier, but the tier
 * assignment stays hardcoded — a missed question costs nothing, a heavy probe
 * on a grief entry is unrecoverable.
 */
export type ProbeTier = 'silent' | 'safe' | 'heavy' | 'retrieval';

// ---------------------------------------------------------------------------
// Relations — §5.4. An edge requires a nameable relation. If the best the
// model can produce is "related" or "same subject", we draw nothing.
// ---------------------------------------------------------------------------

export type Relation =
  | 'contradicts'
  | 'same move'
  | 'returns to'
  | 'questions'
  | 'extends'
  | 'example of'
  | 'echoes';

/**
 * §5.4/§6.2 resolution: no parent→child line exists (§6.2's child entries
 * thicken the parent's ring instead), so fact-vs-guess renders via
 * --edge-fact (accepted/manual) vs --edge (proposed).
 */
export type EdgeStatus = 'proposed' | 'accepted' | 'dismissed' | 'manual';

export interface Edge {
  id: string;
  entryA: string;
  entryB: string;
  relation: Relation;
  /** Optional question carried by the connection — opened from the entry view. */
  question: string | null;
  status: EdgeStatus;
  createdAt: string;
}

// ---------------------------------------------------------------------------
// Spans — §7.3. Attribution is separated so the move vector can exclude
// quoted material; without it, a note quoting three people gets connected on
// *their* thinking.
// ---------------------------------------------------------------------------

export interface Span {
  start: number;
  end: number;
  /** true = quoted from a source, false = the user's own words */
  attributed: boolean;
}

/**
 * §1.2 — an inert type. Never initiates a question, never nags. Ticking is
 * state on the span, not a mutation of the text: the record accumulates,
 * nothing is overwritten.
 */
export interface ActionItem {
  id: string;
  entryId: string;
  /** Anchored to the span it came from, so ticking never edits the transcript. */
  span: Span;
  text: string;
  done: boolean;
}

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

export interface Entry {
  id: string;
  /** null => typed entry. No fingerprint, which is a free visual distinction (§4). */
  audioPath: string | null;
  /** The record. Verbatim, never rewritten, never summarised over (§1.1, §2). */
  transcript: string;
  createdAt: string;

  /** Frozen at insert and never recomputed (§5.1). Position encodes *when*. */
  x: number;
  y: number;

  /** Set => this entry is an answer; it thickens the parent's ring (§6.2). */
  parentEdge: string | null;

  type: RenderedType;
  storedType: StoredType;

  /** User-declared only. The AI never decides you're done thinking (§6.3). */
  resolved: boolean;
  resolutionText: string | null;

  // --- enrichment; added to the record, never replacing it ---

  /** 3–4 words, from the user's own phrasing where possible (§5.2). */
  title: string;
  /**
   * §7.1 — embedded for similarity, and displayable, but never above the
   * transcript. null for felt entries: "reflects on a relationship that ended"
   * is worse than useless (§1.1).
   */
  summary: string | null;

  durationMs: number;
  /** 7–9 samples downsampled from actual amplitude (§5.2). Empty if typed. */
  fingerprint: number[];

  /** §5.3 dashed stroke. From the user's own hedges — regex, not a model. */
  unfinished: boolean;
  /** §9.4 — set by the user at capture, never inferred by a classifier. */
  localOnly: boolean;

  spans: Span[];
  actionItems: ActionItem[];

  /** Marks a seeded sample entry so it can never be mistaken for the user's own. */
  isSample?: boolean;
}

/**
 * The one voice in the app that isn't the user's (§8). Rendered ~15px serif so
 * it visibly isn't theirs. Every analytical claim quotes a span (§3.4), which
 * is what makes it checkable rather than authoritative.
 */
export interface Question {
  id: string;
  entryId: string;
  text: string;
  /** The span the question is anchored to. Unanchored output is not allowed. */
  span: Span | null;
  answered: boolean;
  /** Shown in the UI — the user always knows who answered (§9.4). */
  providerName: string;
  createdAt: string;
}

// ---------------------------------------------------------------------------
// System / settings — drives onboarding and §9.4's model management.
// ---------------------------------------------------------------------------

export interface SystemProfile {
  totalRamBytes: number;
  availableRamBytes: number;
  cpuName: string;
  cpuCores: number;
  gpuName: string | null;
  vramBytes: number | null;
}

export type ModelState =
  | { kind: 'not-downloaded' }
  | { kind: 'downloading'; receivedBytes: number; totalBytes: number }
  | { kind: 'ready' }
  | { kind: 'failed'; error: string };

/** Transcription gates recording; reasoning does not (§9.4). */
export type ModelKind = 'transcription' | 'reasoning';

export interface ModelInfo {
  id: string;
  kind: ModelKind;
  name: string;
  params: string;
  quantization: string;
  sizeBytes: number;
  /** Minimum RAM we'd recommend this at — drives the onboarding default. */
  recommendedRamBytes: number;
  state: ModelState;
}

/** §9.4 — the residency fork. Only the post-recording question is latency-sensitive. */
export type Residency = 'warm' | 'cold';

export interface Settings {
  hotkey: string;
  discardHotkey: string;
  modelId: string | null;
  residency: Residency;
  providerName: string;
  /** Global default for the per-entry local-only flag (§9.4). */
  defaultLocalOnly: boolean;
  transcriptionModel: 'tiny' | 'base' | 'small';
}
