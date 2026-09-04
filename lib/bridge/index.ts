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

  // -- search ---------------------------------------------------------------
  /** Case-insensitive substring search over transcripts. */
  searchEntries(query: string): Promise<SearchHit[]>;

  // -- capture ------------------------------------------------------------
  /**
   * §4: the hotkey starts recording immediately and the panel appears second.
   * Panel-first means two actions and a moment looking at UI before speaking.
   */
  startRecording(): Promise<void>;
  /** Returns the entry once transcription lands (~2s). `parentEdge` makes it
   *  an answer — a layer on that entry rather than its own node (§6.2). */
  stopRecording(parentEdge?: string | null): Promise<Entry>;
  /** Discard belongs in the recording state, not after — you know it's junk
   *  before you stop (§4). Backed by a ~60s undo window, not a dialog. */
  discardRecording(): Promise<void>;
  undoDiscard(): Promise<Entry | null>;
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
   * User-invoked question (§3.6). Which probe fits is the model's call — the
   * UI offers one door, not a menu of techniques. Register does not gate here
   * — §3.2 gives the invoked path to the user — but role and provenance do,
   * because a fact, a list and someone else's sentence offer nothing to push on.
   */
  askQuestion(entryId: string, span?: Span | null): Promise<Question>;
  /** The primitive askQuestion picks from. Kept for replay and evaluation;
   *  no UI path names a probe. */
  runProbe(entryId: string, probeId: string, span?: Span | null): Promise<Question>;
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
