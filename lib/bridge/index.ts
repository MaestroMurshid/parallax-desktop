/**
 * The bridge — the one seam between UI and everything below it (§9.1): this
 * interface is both what the UI codes against and the spec for Rust's future
 * command surface. Import only via getBridge(), never ./tauri or ./mock directly.
 */

import type {
  ActionItem,
  Edge,
  Entry,
  ModelInfo,
  Question,
  Settings,
  Span,
  SystemProfile,
} from '@/lib/types';

/** Unsubscribe. Every stream returns one; call it on unmount. */
export type Unsubscribe = () => void;

/** One match from searchEntries. Offsets are into the transcript;
 *  snippetStart/snippetEnd are the same match re-based into `snippet`. */
export interface SearchHit {
  entryId: string;
  start: number;
  end: number;
  /** ~90 chars of surrounding transcript, for display. */
  snippet: string;
  snippetStart: number;
  snippetEnd: number;
}

export interface NewEntryDraft {
  transcript: string;
  durationMs: number;
  fingerprint: number[];
  /** Set when this is an answer to a question — makes it a thread layer (§6.2). */
  parentEdge?: string | null;
  localOnly?: boolean;
  /** true for the typed-entry path (§4). Nobody dictates a list. */
  typed?: boolean;
}

export interface Bridge {
  readonly kind: 'mock' | 'tauri';

  // -- corpus -------------------------------------------------------------
  listEntries(): Promise<Entry[]>;
  getEntry(id: string): Promise<Entry | null>;
  /** Children of an entry — the layers behind its rings (§6.2). */
  listChildren(entryId: string): Promise<Entry[]>;
  listEdges(): Promise<Edge[]>;
  createEntry(draft: NewEntryDraft): Promise<Entry>;
  /** Manual placement override (§5.1). Auto-placement stays the default; this
   *  just overwrites the frozen position, it never re-solves the field. */
  moveEntry(id: string, x: number, y: number): Promise<Entry>;
  /** Removes one entry and any edges touching it. Children are orphaned, not
   *  deleted — an answer is still something you said (§6.2). */
  deleteEntry(id: string): Promise<void>;
  /**
   * Fixes what the speech-to-text heard wrong. The *only* edit a note takes:
   * a note is the verbatim record of what was said, so there is no append and
   * no rewrite — a note you can reword is a note you cannot cite.
   *
   * Returns the entry because the correction re-anchors it. Spans, questions
   * and action items are keyed on offsets into the transcript, so Rust re-finds
   * each one by its stored quote and drops the spans whose words are genuinely
   * gone; the caller must take the entry that comes back rather than patching
   * the transcript locally and keeping the old offsets. Rejects a blank
   * transcript — emptying a note is a delete, not a correction.
   */
  correctTranscript(entryId: string, transcript: string): Promise<Entry>;

  // -- search ---------------------------------------------------------------
  /**
   * Case-insensitive search over transcripts. Substring by default, because
   * most searches are for a phrase half-remembered. A query wrapped in double
   * quotes matches whole words only — the way to ask for a subject rather than
   * any word containing it (`ai` otherwise finds maintain and explaining).
   */
  searchEntries(query: string): Promise<SearchHit[]>;

  // -- capture ------------------------------------------------------------
  /**
   * §4: the hotkey starts recording immediately and the panel appears second.
   * Panel-first means two actions and a moment looking at UI before speaking.
   */
  startRecording(): Promise<void>;
  /** Returns the entry once transcription lands (~2s). `parentEdge` makes it
   *  an answer — a layer on that entry rather than its own node (§6.2). */
  stopRecording(parentEdge?: string | null, questionId?: string | null): Promise<Entry>;
  /** Discard belongs in the recording state, not after — you know it's junk
   *  before you stop (§4). Backed by a ~60s undo window, not a dialog. */
  discardRecording(): Promise<void>;
  undoDiscard(): Promise<Entry | null>;
  /** The transcript so far, while still recording. Empty when there is no model
   *  yet, too little audio, or nothing recording. */
  partialTranscript(): Promise<string>;
  /** Live amplitude for the equalizer bars. The only thing that animates (§8). */
  onAmplitude(cb: (level: number) => void): Unsubscribe;

  // -- enrichment ---------------------------------------------------------
  /**
   * Auto post-recording question; resolves to null when any of the three
   * facets suppresses it — not a position, live register, someone else's
   * words, or under ~30s (§3.2). A missed question beats a bad probe.
   */
  getQuestion(entryId: string): Promise<Question | null>;
  /**
   * Every question in the corpus, in one call. `getQuestion` returns only the
   * oldest open one, so a load built on it drops every answered and dismissed
   * question from the record and from the export — the accumulation §3.4 is
   * about. Optional because the mock holds questions per entry in memory and
   * has nothing to restore; the load path falls back to `getQuestion` without it.
   */
  listQuestions?(): Promise<Question[]>;
  /**
   * User-invoked question (§3.6). Which probe fits is the model's call — the
   * UI offers one door, not a menu of techniques. Register does not gate here
   * — §3.2 gives the invoked path to the user — but role and provenance do,
   * because a fact, a list and someone else's sentence offer nothing to push on.
   */
  askQuestion(entryId: string, span?: Span | null): Promise<Question>;
  /** The primitive askQuestion picks from. Kept for replay and evaluation;
   *  no UI path names a probe. */
  runProbe(entryId: string, probeId: string, span?: Span | null): Promise<Question>;
  /** Strikes a question out. It stays on the entry; it stops being open. */
  dismissQuestion(entryId: string, questionId: string): Promise<void>;
  /** Proposed connections, shown as dismissible cards below the transcript (§6.1). */
  listProposedEdges(entryId: string): Promise<Edge[]>;
  /** Dismissals are training signal, not just UI (§6.1). */
  dismissEdge(edgeId: string): Promise<void>;
  acceptEdge(edgeId: string): Promise<void>;
  /** §5.4 — with a high threshold the app will miss real connections, and
   *  naming one yourself is the step the research says carries the benefit. */
  createManualEdge(a: string, b: string, relation: Edge['relation']): Promise<Edge>;

  // -- action items (§1.2) ------------------------------------------------
  listActionItems(): Promise<ActionItem[]>;
  /** State on the span, not a mutation of the text. */
  setActionItemDone(id: string, done: boolean): Promise<void>;

  // -- resolution (§6.3) --------------------------------------------------
  /** Requires stating what the resolution *is*. A bare flag gives the app nothing. */
  resolveEntry(entryId: string, text: string): Promise<Entry>;
  reopenEntry(entryId: string): Promise<Entry>;

  // -- system / onboarding ------------------------------------------------
  /** Drives the recommended-model default so onboarding stays one screen. */
  getSystemProfile(): Promise<SystemProfile>;
  listModels(): Promise<ModelInfo[]>;
  /** Downloads in the background; gates nothing. Capture and transcription
   *  work without it, and the question surfaces when the model lands (§9.4). */
  downloadModel(modelId: string): Promise<void>;
  onModelProgress(cb: (m: ModelInfo) => void): Unsubscribe;
  /** Fires when an enrichment pass starts. Paired with `onEntryEnriched`,
   *  which fires on every exit including failure -- an indicator that only
   *  clears on success is an indicator that sticks. */
  onEntryEnriching(cb: (entryId: string) => void): Unsubscribe;
  /** Fires when classification and the question have landed on an entry.
   *  Enrichment runs after capture returns, so without this the canvas keeps
   *  showing the placeholder title and the question never appears. */
  onEntryEnriched(cb: (entryId: string) => void): Unsubscribe;
  /** The recording itself, for playback. `null` when the entry was typed or the
   *  file is gone -- the caller falls back to a simulated clock. */
  readAudio(entryId: string): Promise<ArrayBuffer | null>;

  getSettings(): Promise<Settings>;
  setSettings(patch: Partial<Settings>): Promise<Settings>;

  // -- sample corpus ------------------------------------------------------
  /** Offered from the empty state, never forced. Sample entries stay marked
   *  so they can never be mistaken for the user's own. */
  loadSampleCorpus(): Promise<void>;
  clearSampleCorpus(): Promise<void>;

  /** Restores a previously exported corpus. 'merge' keeps existing ids. */
  importCorpus(data: CorpusImport, mode: ImportMode): Promise<void>;
}

let instance: Bridge | null = null;

/** True when running inside the Tauri webview rather than a browser tab. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function getBridge(): Bridge {
  if (instance) return instance;
  throw new Error('Bridge not initialised — call initBridge() first.');
}

/**
 * Which implementation runs. NEXT_PUBLIC_BRIDGE=mock forces the fixture
 * backend even inside Tauri (pre-Rust UI phase) — explicit rather than a
 * silent fallback, which is how you ship a stub by accident (§9.4).
 */
export async function initBridge(): Promise<Bridge> {
  if (instance) return instance;
  const forceMock = process.env.NEXT_PUBLIC_BRIDGE === 'mock';
  if (isTauri() && !forceMock) {
    const { TauriBridge } = await import('./tauri');
    instance = new TauriBridge();
  } else {
    const { MockBridge } = await import('./mock');
    instance = new MockBridge();
  }
  return instance;
}

/** Test seam — lets a story or a test pin a specific implementation. */
export function __setBridge(b: Bridge): void {
  instance = b;
}

export type ImportMode = 'merge' | 'replace';

export interface CorpusImport {
  entries: Entry[];
  edges: Edge[];
  questions: Question[];
}
