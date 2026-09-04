/**
 * MockBridge — the whole backend, faked, behind the real interface (§9.1) so
 * the frontend runs before Rust exists. Fakes real shape: actual §5.1
 * placement, ~2s §4 transcription delay, real amplitude events.
 */

import type {
  ActionItem,
  Edge,
  Entry,
  ModelInfo,
  Question,
  Role,
  Settings,
  SystemProfile,
  Span,
} from '@/lib/types';
import { radiusForDuration } from '@/lib/scene/blob';
import { automaticProbes, invokedProbes, mayProbeAutomatically } from '@/lib/scene/classification';
import { detectUnfinished } from '@/lib/scene/markers';
import { placeEntry, type PlacedNode } from '@/lib/scene/placement';
import { titleSizeForDuration, wrapTitle } from '@/lib/scene/lexicon';
import { hash32, mockVector, rng } from '@/lib/scene/vector';
import { loadSeedCorpus } from '@/fixtures/load';
import type {
  Bridge,
  CorpusImport,
  ImportMode,
  NewEntryDraft,
  SearchHit,
  Unsubscribe,
} from './index';

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

const SETTINGS_KEY = 'parallax.settings';
const POSITIONS_KEY = 'parallax.positions';

const SEARCH_SNIPPET_RADIUS = 40;
const SEARCH_MAX_HITS = 50;
const SEARCH_MAX_PER_ENTRY = 3;

/** Deliberative speech with its pauses — the rate the seed corpus is timed at. */
const SPEECH_WORDS_PER_SEC = 1.15;

interface PlaceholderNote {
  /**
   * Stand-in for transcribe.cpp output. Written as speech, not tidy prose — a
   * clean paragraph would misrepresent what the corpus actually looks like
   * (§2: the digression is the content).
   */
  transcript: string;
  /** What the classifier would have decided (§3.6). The mock stands in for it. */
  role: Role;
  /**
   * The question §7.2's background pass would have written, and the sentence it
   * anchors to. Authored rather than left to the probe library so a live take
   * shows the real thing, and so it lands on the corpus it was recorded into.
   */
  question: { text: string; quote: string };
}

const PLACEHOLDER_NOTES: PlaceholderNote[] = [
  {
    role: 'position',
    transcript:
      "Okay so I keep going back and forth on this. Every time a model tops another benchmark I catch myself updating, and then a week later it turns out the thing was in the training data somewhere. I don't think I actually know how to tell the difference between a system getting better and a system having seen the answer, and that seems like a problem for how I read any of these results.",
    question: {
      text: 'The update happens before you know which one it was. What would you have to see, at the moment you read a result, to tell a system that got better from one that had already seen the answer?',
      quote:
        'how to tell the difference between a system getting better and a system having seen the answer',
    },
  },
  {
    role: 'evidence',
    transcript:
      "Right, quick thing before I forget. Cache invalidation isn't hard because the code is hard, it's hard because the cache is a second copy of the truth and nobody agrees on who owns it. Same shape as the index thing — you buy speed with a copy, and then the copy is the problem.",
    question: {
      text: 'You have now called this the same shape as the index trade-off. Take a case where the second copy is not the problem — what is different about it?',
      quote: 'you buy speed with a copy, and then the copy is the problem',
    },
  },
  {
    role: 'position',
    transcript:
      "I've noticed I only ever ask for a second opinion after I've already decided. Not before, which is when it would actually be worth something. And I'm not sure whether that's me looking for permission or me checking my work, and I think the honest answer is that it depends on the day.",
    question: {
      text: 'A second opinion that arrives after the decision can only confirm it or annoy you. What would have to be true for you to ask before?',
      quote: "I only ever ask for a second opinion after I've already decided",
    },
  },
];

/** Anchors an authored question to its sentence. Every claim quotes a span (§3.4). */
function spanOf(transcript: string, quote: string): Span | null {
  const start = transcript.indexOf(quote);
  return start === -1 ? null : { start, end: start + quote.length, attributed: false };
}

/** A live entry has no title until transcription lands, so assume a typical one. */
function liveBox(durationMs: number, title = 'four word placeholder title'): { halfW: number; halfH: number } {
  const size = titleSizeForDuration(durationMs);
  const lines = wrapTitle(title);
  const longest = lines.reduce((m, l) => Math.max(m, l.length), 0);
  return { halfW: (longest * size * 0.55) / 2, halfH: (lines.length * size * 1.13) / 2 };
}

export class MockBridge implements Bridge {
  readonly kind = 'mock' as const;

  private entries = new Map<string, Entry>();
  private edges: Edge[] = [];
  /** Per entry, in the order they were asked. Nothing is ever replaced. */
  private questions = new Map<string, Question[]>();
  private actionItems: ActionItem[] = [];

  private amplitudeListeners = new Set<(level: number) => void>();
  private modelListeners = new Set<(m: ModelInfo) => void>();
  private amplitudeTimer: ReturnType<typeof setInterval> | null = null;
  private recordingStartedAt = 0;
  private discarded: Entry | null = null;
  private seq = 0;
  /** Manual placement overrides (§5.1), stands in for the persisted x/y column. */
  private overrides: Record<string, { x: number; y: number }> = readOverrides();
  /** Which stand-in the next take gets. */
  private liveTake = 0;

  private settings: Settings = {
    hotkey: 'Ctrl+Shift+Space',
    discardHotkey: 'Escape',
    modelId: null,
    residency: 'warm',
    providerName: 'llama-server',
    defaultLocalOnly: false,
    transcriptionModel: 'base',
  };

  private models: ModelInfo[] = [
    {
      id: 'whisper-tiny',
      kind: 'transcription',
      name: 'tiny',
      params: '39M',
      quantization: 'q5_1',
      sizeBytes: 75_000_000,
      recommendedRamBytes: 2e9,
      state: { kind: 'not-downloaded' },
    },
    {
      id: 'whisper-base',
      kind: 'transcription',
      name: 'base',
      params: '74M',
      quantization: 'q5_1',
      sizeBytes: 140_000_000,
      recommendedRamBytes: 4e9,
      state: { kind: 'not-downloaded' },
    },
    {
      id: 'whisper-small',
      kind: 'transcription',
      name: 'small',
      params: '244M',
      quantization: 'q5_1',
      sizeBytes: 470_000_000,
      recommendedRamBytes: 8e9,
      state: { kind: 'not-downloaded' },
    },
    {
      id: 'qwen3-1.7b-q4',
      kind: 'reasoning',
      name: 'Qwen3 1.7B',
      params: '1.7B',
      quantization: 'Q4_K_M',
      sizeBytes: 1_050_000_000,
      recommendedRamBytes: 8e9,
      state: { kind: 'not-downloaded' },
    },
    {
      id: 'qwen3-4b-q4',
      kind: 'reasoning',
      name: 'Qwen3 4B',
      params: '4B',
      quantization: 'Q4_K_M',
      sizeBytes: 2_400_000_000,
      recommendedRamBytes: 16e9,
      state: { kind: 'not-downloaded' },
    },
    {
      id: 'qwen3-8b-q4',
      kind: 'reasoning',
      name: 'Qwen3 8B',
      params: '8B',
      quantization: 'Q4_K_M',
      sizeBytes: 4_700_000_000,
      recommendedRamBytes: 32e9,
      state: { kind: 'not-downloaded' },
    },
  ];

  // -- corpus -------------------------------------------------------------

  async listEntries(): Promise<Entry[]> {
    return [...this.entries.values()];
  }

  async getEntry(id: string): Promise<Entry | null> {
    return this.entries.get(id) ?? null;
  }

  async listChildren(entryId: string): Promise<Entry[]> {
    return [...this.entries.values()]
      .filter((e) => e.parentEdge === entryId)
      .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  }

  async listEdges(): Promise<Edge[]> {
    return [...this.edges];
  }

  async createEntry(draft: NewEntryDraft): Promise<Entry> {
    const id = `entry-${Date.now()}-${this.seq++}`;
    const box = liveBox(draft.durationMs);

    // Real placement against the real field (§5.1). Nothing already placed moves.
    const field: PlacedNode[] = [];
    const vectors = new Map<string, readonly number[]>();
    for (const e of this.entries.values()) {
      if (e.parentEdge !== null) continue;
      field.push({ id: e.id, x: e.x, y: e.y, ...liveBox(e.durationMs, e.title), isolated: false });
      vectors.set(e.id, mockVector(e.typeId, e.id));
    }
    const placed = placeEntry({ id, vec: mockVector('live-capture', id), ...box }, field, vectors);

    const entry: Entry = {
      id,
      audioPath: draft.typed ? null : `mock://${id}.wav`,
      transcript: draft.transcript,
      createdAt: new Date().toISOString(),
      x: placed.x,
      y: placed.y,
      parentEdge: draft.parentEdge ?? null,
      // A fresh capture before the classifier has seen it. `neutral` is not a
      // guess about the speaker — nothing has run yet, and the automatic
      // question is gated on role and duration too.
      role: 'position',
      register: 'neutral',
      typeId: 'position',
      answersQuestionId: null,
      resolved: false,
      resolutionText: null,
      title: deriveTitle(draft.transcript),
      summary: null,
      durationMs: draft.durationMs,
      fingerprint: draft.typed ? [] : draft.fingerprint,
      unfinished: detectUnfinished(draft.transcript),
      localOnly: draft.localOnly ?? this.settings.defaultLocalOnly,
      spans: [],
      actionItems: [],
    };
    this.entries.set(id, entry);
    return entry;
  }

  async deleteEntry(id: string): Promise<void> {
    this.entries.delete(id);
    this.edges = this.edges.filter((e) => e.entryA !== id && e.entryB !== id);
    this.questions.delete(id);
    this.actionItems = this.actionItems.filter((a) => a.entryId !== id);
    delete this.overrides[id];
    // Children keep their text but lose the parent link rather than vanishing.
    for (const [cid, e] of this.entries) {
      if (e.parentEdge === id) this.entries.set(cid, { ...e, parentEdge: null });
    }
  }

  async moveEntry(id: string, x: number, y: number): Promise<Entry> {
    const entry = this.entries.get(id);
    if (!entry) throw new Error(`No entry ${id}`);
    const next: Entry = { ...entry, x, y };
    this.entries.set(id, next);
    this.overrides[id] = { x, y };
    this.saveOverrides();
    return next;
  }

  /** Frozen positions outlive the in-memory corpus, so every path that changes
   *  them writes through here rather than remembering the key. */
  private saveOverrides(): void {
    try {
      localStorage.setItem(POSITIONS_KEY, JSON.stringify(this.overrides));
    } catch {
      /* blocked storage — the move still applies for this session */
    }
  }

  // -- search ---------------------------------------------------------------

  async searchEntries(query: string): Promise<SearchHit[]> {
    const raw = query.trim();
    // A quoted query matches whole words only. Substring is the right default
    // for a phrase you half-remember, and exactly wrong for a subject: bare
    // `ai` finds maintain, explaining and failures before it finds an entry
    // about AI. Quoting is the way to say you meant the word.
    const quoted = raw.length > 2 && raw.startsWith('"') && raw.endsWith('"');
    const q = (quoted ? raw.slice(1, -1) : raw).trim().toLowerCase();
    if (!q) return [];
    const hits: SearchHit[] = [];
    for (const entry of this.entries.values()) {
      if (hits.length >= SEARCH_MAX_HITS) break;
      const lower = entry.transcript.toLowerCase();
      let from = 0;
      let found = 0;
      while (found < SEARCH_MAX_PER_ENTRY && hits.length < SEARCH_MAX_HITS) {
        const start = lower.indexOf(q, from);
        if (start === -1) break;
        const end = start + q.length;
        if (quoted && !wordBounded(lower, start, end)) {
          // Step past this occurrence only — the next one may stand alone.
          from = start + 1;
          continue;
        }
        hits.push({ entryId: entry.id, start, end, ...buildSnippet(entry.transcript, start, end) });
        found++;
        from = end;
      }
    }
    return hits;
  }

  // -- capture ------------------------------------------------------------

  async startRecording(): Promise<void> {
    this.recordingStartedAt = Date.now();
    const r = rng(hash32(String(this.recordingStartedAt)));
    let level = 0.3;
    // ~30Hz is enough for bars that read as responsive without being noise.
    this.amplitudeTimer = setInterval(() => {
      // Random walk with occasional pauses — speech, not a sine wave.
      const target = r() < 0.08 ? 0.06 : 0.25 + r() * 0.7;
      level += (target - level) * 0.35;
      const out = Math.min(1, Math.max(0, level));
      for (const cb of this.amplitudeListeners) cb(out);
    }, 33);
  }

  async stopRecording(
    parentEdge: string | null = null,
    questionId: string | null = null,
  ): Promise<Entry> {
    const held = Date.now() - this.recordingStartedAt;
    this.stopAmplitude();
    await sleep(1800); // §4 — transcription runs (~2s)

    // Cycle rather than sample, so recording twice in a demo gives two notes.
    const note = PLACEHOLDER_NOTES[this.liveTake++ % PLACEHOLDER_NOTES.length]!;
    const words = note.transcript.split(/\s+/).length;
    // What the stand-in transcript would have taken to say. Holding the hotkey
    // for four seconds and getting back seventy words is already a fiction; the
    // wall clock also puts every take under §3.2's thirty-second line, so the
    // one thing a live take is supposed to demonstrate never fires. A real hold
    // longer than the text still keeps its own length.
    const durationMs = Math.max(held, Math.round(words / SPEECH_WORDS_PER_SEC) * 1000);
    const r = rng(hash32(String(held)));
    const fingerprint = Array.from({ length: 8 }, () => 0.15 + r() * 0.85);

    const entry = await this.createEntry({
      transcript: note.transcript,
      durationMs,
      fingerprint,
      parentEdge,
    });
    entry.answersQuestionId = questionId;
    // Stands in for the classifier (§3.6) — createEntry cannot know this, and
    // the role is what decides which probe the entry is even eligible for.
    entry.role = note.role;
    entry.typeId = note.role;
    this.entries.set(entry.id, entry);

    // The question §7.2 would have written during enrichment. getQuestion still
    // applies the §3.2 gate over it, so this never bypasses suppression.
    this.questions.set(entry.id, [
      {
        id: `question-live-${entry.id}`,
        entryId: entry.id,
        text: note.question.text,
        span: spanOf(note.transcript, note.question.quote),
        answered: false,
        dismissed: false,
        providerName: this.settings.providerName,
        createdAt: new Date().toISOString(),
      },
    ]);

    // An answer is a note in its own right, not a turn in a conversation. It
    // lands on the canvas, carries a drawn line back to what it answers, and
    // is eligible for its own question like anything else you say.
    if (parentEdge && this.entries.has(parentEdge)) {
      this.edges.push({
        id: `edge-answer-${entry.id}`,
        entryA: parentEdge,
        entryB: entry.id,
        relation: 'answers',
        question: null,
        status: 'accepted',
        createdAt: new Date().toISOString(),
      });
    }
    return entry;
  }

  async discardRecording(): Promise<void> {
    this.stopAmplitude();
    this.discarded = null;
  }

  async undoDiscard(): Promise<Entry | null> {
    return this.discarded;
  }

  onAmplitude(cb: (level: number) => void): Unsubscribe {
    this.amplitudeListeners.add(cb);
    return () => {
      this.amplitudeListeners.delete(cb);
    };
  }

  private stopAmplitude(): void {
    if (this.amplitudeTimer) clearInterval(this.amplitudeTimer);
    this.amplitudeTimer = null;
    for (const cb of this.amplitudeListeners) cb(0);
  }

  // -- enrichment ---------------------------------------------------------

  async getQuestion(entryId: string): Promise<Question | null> {
    const entry = this.entries.get(entryId);
    if (!entry) return null;
    // §3.2 suppression re-enforced here rather than trusted upstream.
    if (!mayProbeAutomatically(entry)) return null;
    const existing = this.questions.get(entryId)?.[0];
    if (existing) return existing;
    // Stands in for §7.2's background pass: a real backend generates and stores
    // during enrichment and this call only reads. Generating lazily here keeps
    // the invariant — anything the gate lets through has a question — without
    // it depending on whether one was written into the fixture. The seed corpus
    // still supplies the better-written examples where it has them.
    //
    // Restricted to the Safe tier. Reaching for invokedProbes here fires a
    // steelman as the opening move, which §3.2 forbids.
    const probe = automaticProbes(entry)[0];
    return probe ? this.runProbe(entryId, probe.id) : null;
  }

  /**
   * Stand-in for a Heavy-tier probe. Real ones are one llama-server call with
   * the §3.4 stance rules; these keep the shape — one question, span-anchored,
   * third person about the entry, never a verdict.
   */
  async askQuestion(entryId: string, span?: Span | null): Promise<Question> {
    const entry = this.entries.get(entryId);
    if (!entry) throw new Error(`No entry ${entryId}`);
    // Stands in for the model's choice: the eligible set, then the first that
    // fits. Real inference would weigh the transcript, not the order.
    const eligible = invokedProbes(entry);
    // Rotate rather than always returning the first: questions accumulate now,
    // so asking twice about different sentences should not repeat itself.
    const chosen = eligible[(this.questions.get(entryId)?.length ?? 0) % eligible.length];
    if (!chosen) throw new Error(`Nothing to ask of ${entryId}`);
    return this.runProbe(entryId, chosen.id, span);
  }

  async runProbe(entryId: string, probeId: string, span?: Span | null): Promise<Question> {
    const entry = this.entries.get(entryId);
    if (!entry) throw new Error(`No entry ${entryId}`);
    await sleep(900);

    // Facet 3: the app may only push on the user's own words. The gate lets a
    // mixed entry through — most notes about a book contain a real position —
    // so the anchor has to be chosen, not assumed. Without this the steelman
    // lands on the author's sentence and asks you to defend someone else's book.
    const attributed = entry.spans.filter((sp) => sp.attributed);
    const borrowed = (from: number, to: number) =>
      attributed.some((sp) => from < sp.end && to > sp.start);

    const candidates = entry.transcript
      .split(/(?<=[.?!])\s+/)
      .filter((x) => x.length > 24)
      .map((text) => ({ text, start: entry.transcript.indexOf(text) }))
      .filter((c) => c.start >= 0 && !borrowed(c.start, c.start + c.text.length));

    // A span the user selected wins: they already said what this is about.
    const chosen = candidates[Math.min(1, candidates.length - 1)];
    const pick = span
      ? entry.transcript.slice(span.start, span.end)
      : chosen?.text ?? entry.transcript.slice(0, 90);
    const start = span ? span.start : chosen ? chosen.start : entry.transcript.indexOf(pick);
    const text: Record<string, string> = {
      steelman:
        'Put at its strongest, the entry says the constraint is structural rather than chosen. Does the argument still need the weaker version it actually makes?',
      boundary:
        'The entry states this generally. Where is the first case you would expect it to stop holding?',
      disconfirming:
        'What would you have to see for the entry to be wrong about this?',
      munchhausen:
        'The entry rests on that being the case. What is that resting on?',
      // A real model writes this against the transcript, so it lands on the
      // subject: a case the concept has to cover, a consequence to follow. This
      // stub cannot do that and should not be read as the shape of the mode.
      feynman:
        'Take a case this would have to cover that has not come up yet. Does it still hold?',
    };

    const question: Question = {
      id: `probe-${probeId}-${entryId}-${this.seq++}`,
      entryId,
      text: text[probeId] ?? 'What is this resting on?',
      span: start >= 0 ? { start, end: start + pick.length, attributed: false } : null,
      answered: false,
      dismissed: false,
      providerName: `${this.settings.providerName} · ${probeId}`,
      createdAt: new Date().toISOString(),
    };
    this.questions.set(entryId, [...(this.questions.get(entryId) ?? []), question]);
    return question;
  }

  async dismissQuestion(entryId: string, questionId: string): Promise<void> {
    const prior = this.questions.get(entryId);
    if (!prior) return;
    this.questions.set(
      entryId,
      prior.map((q) => (q.id === questionId ? { ...q, dismissed: true } : q)),
    );
  }

  async listProposedEdges(entryId: string): Promise<Edge[]> {
    return this.edges.filter(
      (e) => e.status === 'proposed' && (e.entryA === entryId || e.entryB === entryId),
    );
  }

  async dismissEdge(edgeId: string): Promise<void> {
    this.edges = this.edges.map((e) => (e.id === edgeId ? { ...e, status: 'dismissed' } : e));
  }

  async acceptEdge(edgeId: string): Promise<void> {
    this.edges = this.edges.map((e) => (e.id === edgeId ? { ...e, status: 'accepted' } : e));
  }

  async createManualEdge(a: string, b: string, relation: Edge['relation']): Promise<Edge> {
    const edge: Edge = {
      id: `edge-manual-${this.seq++}`,
      entryA: a,
      entryB: b,
      relation,
      question: null,
      status: 'manual',
      createdAt: new Date().toISOString(),
    };
    this.edges.push(edge);
    return edge;
  }

  // -- action items -------------------------------------------------------

  async listActionItems(): Promise<ActionItem[]> {
    return [...this.actionItems];
  }

  async setActionItemDone(id: string, done: boolean): Promise<void> {
    this.actionItems = this.actionItems.map((a) => (a.id === id ? { ...a, done } : a));
  }

  // -- resolution ---------------------------------------------------------

  async resolveEntry(entryId: string, text: string): Promise<Entry> {
    const entry = this.entries.get(entryId);
    if (!entry) throw new Error(`No entry ${entryId}`);
    const next: Entry = { ...entry, resolved: true, resolutionText: text };
    this.entries.set(entryId, next);
    return next;
  }

  async reopenEntry(entryId: string): Promise<Entry> {
    const entry = this.entries.get(entryId);
    if (!entry) throw new Error(`No entry ${entryId}`);
    const next: Entry = { ...entry, resolved: false };
    this.entries.set(entryId, next);
    return next;
  }

  // -- system -------------------------------------------------------------

  async getSystemProfile(): Promise<SystemProfile> {
    const nav = navigator as Navigator & { deviceMemory?: number };
    const gb = nav.deviceMemory ?? 16;
    return {
      totalRamBytes: gb * 1e9,
      availableRamBytes: gb * 0.55 * 1e9,
      cpuName: 'Detected by sysinfo on the Rust side',
      cpuCores: nav.hardwareConcurrency ?? 8,
      gpuName: null,
      vramBytes: null,
    };
  }

  async listModels(): Promise<ModelInfo[]> {
    return this.models.map((m) => ({ ...m }));
  }

  async downloadModel(modelId: string): Promise<void> {
    const model = this.models.find((m) => m.id === modelId);
    if (!model) throw new Error(`No model ${modelId}`);
    let received = 0;
    // Downloads in the background and gates nothing — capture and transcription
    // work without it, and the question surfaces when the model lands (§9.4).
    // Ticks scale with size, so the small transcription model lands first.
    const ticks = Math.max(6, Math.round(model.sizeBytes / 1.2e8));
    const step = model.sizeBytes / ticks;
    const timer = setInterval(() => {
      received = Math.min(model.sizeBytes, received + step);
      model.state =
        received >= model.sizeBytes
          ? { kind: 'ready' }
          : { kind: 'downloading', receivedBytes: received, totalBytes: model.sizeBytes };
      for (const cb of this.modelListeners) cb({ ...model });
      if (received >= model.sizeBytes) clearInterval(timer);
    }, 220);
  }

  onModelProgress(cb: (m: ModelInfo) => void): Unsubscribe {
    this.modelListeners.add(cb);
    return () => {
      this.modelListeners.delete(cb);
    };
  }

  async getSettings(): Promise<Settings> {
    // Stands in for the settings table, so onboarding runs once as it would.
    try {
      const stored = localStorage.getItem(SETTINGS_KEY);
      if (stored) this.settings = { ...this.settings, ...JSON.parse(stored) };
    } catch {
      /* private mode or blocked storage — fall back to defaults */
    }
    return { ...this.settings };
  }

  async setSettings(patch: Partial<Settings>): Promise<Settings> {
    this.settings = { ...this.settings, ...patch };
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(this.settings));
    } catch {
      /* ignore */
    }
    return { ...this.settings };
  }

  // -- sample corpus ------------------------------------------------------

  async loadSampleCorpus(): Promise<void> {
    const seeded = loadSeedCorpus();
    for (const e of seeded.entries) {
      const o = this.overrides[e.id];
      this.entries.set(e.id, o ? { ...e, x: o.x, y: o.y } : e);
    }
    // Idempotent: the sample is offered from the empty state and from settings,
    // and seeded ids are stable, so a second load must not append a second copy
    // of every edge and task. A repeated edge id is also a repeated React key,
    // which strands label elements at the overlay origin.
    const knownEdges = new Set(this.edges.map((e) => e.id));
    this.edges = [...this.edges, ...seeded.edges.filter((e) => !knownEdges.has(e.id))];
    for (const q of seeded.questions) this.questions.set(q.entryId, [q]);
    const knownItems = new Set(this.actionItems.map((a) => a.id));
    this.actionItems = [
      ...this.actionItems,
      ...seeded.actionItems.filter((a) => !knownItems.has(a.id)),
    ];
  }

  async importCorpus(data: CorpusImport, mode: ImportMode): Promise<void> {
    if (mode === 'replace') {
      this.entries.clear();
      this.edges = [];
      this.questions.clear();
      this.actionItems = [];
      this.overrides = {};
      this.saveOverrides();
    }
    for (const e of data.entries) {
      if (mode === 'merge' && this.entries.has(e.id)) continue;
      this.entries.set(e.id, e);
    }
    const known = new Set(this.edges.map((e) => e.id));
    for (const edge of data.edges) {
      if (known.has(edge.id)) continue;
      if (!this.entries.has(edge.entryA) || !this.entries.has(edge.entryB)) continue;
      this.edges.push(edge);
    }
    for (const q of data.questions) {
      if (mode === 'merge' && this.questions.has(q.entryId)) continue;
      if (this.entries.has(q.entryId)) this.questions.set(q.entryId, [q]);
    }
    // Action items live on the entry, so the flat list is rebuilt rather than merged.
    this.actionItems = [...this.entries.values()].flatMap((e) => e.actionItems ?? []);
  }

  async clearSampleCorpus(): Promise<void> {
    for (const [id, e] of this.entries) {
      if (!e.isSample) continue;
      this.entries.delete(id);
      // Or re-loading the sample restores it where you had dragged it in a
      // corpus you have since cleared.
      delete this.overrides[id];
    }
    // An answer to a sample entry is not itself a sample entry, so it survives
    // the sweep above and is left pointing at a parent that is gone (sec 6.2).
    for (const [id, e] of this.entries) {
      if (e.parentEdge !== null && !this.entries.has(e.parentEdge)) {
        this.entries.delete(id);
        delete this.overrides[id];
      }
    }
    this.edges = this.edges.filter((e) => this.entries.has(e.entryA) && this.entries.has(e.entryB));
    for (const [id] of this.questions) if (!this.entries.has(id)) this.questions.delete(id);
    this.actionItems = this.actionItems.filter((a) => this.entries.has(a.entryId));
    this.saveOverrides();
  }
}

function readOverrides(): Record<string, { x: number; y: number }> {
  try {
    return JSON.parse(localStorage.getItem(POSITIONS_KEY) ?? '{}');
  } catch {
    return {};
  }
}

/** ~40 chars either side of a match, trimmed to word boundaries, ellipsis when cut. */
/** True when [start, end) is not sitting inside a longer word. Input is already
 *  lower-cased, so letters are a-z; anything else counts as a boundary. */
function wordBounded(text: string, start: number, end: number): boolean {
  const wordish = /[a-z0-9]/;
  const before = start > 0 ? text[start - 1]! : ' ';
  const after = end < text.length ? text[end]! : ' ';
  return !wordish.test(before) && !wordish.test(after);
}

function buildSnippet(
  text: string,
  matchStart: number,
  matchEnd: number,
): { snippet: string; snippetStart: number; snippetEnd: number } {
  let from = Math.max(0, matchStart - SEARCH_SNIPPET_RADIUS);
  let to = Math.min(text.length, matchEnd + SEARCH_SNIPPET_RADIUS);

  while (from > 0 && !/\s/.test(text[from - 1] ?? '')) from++;
  while (to < text.length && !/\s/.test(text[to] ?? '')) to--;
  // Word-boundary trimming must never eat into the match itself.
  from = Math.min(from, matchStart);
  to = Math.max(to, matchEnd);

  const prefix = from > 0 ? '… ' : '';
  const suffix = to < text.length ? ' …' : '';
  return {
    snippet: prefix + text.slice(from, to) + suffix,
    snippetStart: matchStart - from + prefix.length,
    snippetEnd: matchEnd - from + prefix.length,
  };
}

/**
 * Stand-in for the generated title (§5.2: 3–4 words, the user's own phrasing).
 * Takes opening content words as a crude proxy — the real version is an LLM call.
 */
function deriveTitle(transcript: string): string {
  const stop = new Set([
    'okay', 'so', 'the', 'thing', 'a', 'and', 'but', 'is', 'it', 'that',
    'to', 'of', 'right', 'just', 'about', 'was', 'not', 'this', 'all',
  ]);
  const words = transcript
    .toLowerCase()
    .replace(/[^a-z0-9\s']/g, ' ')
    .split(/\s+/)
    .filter((w) => w.length > 2 && !stop.has(w));
  return words.slice(0, 4).join(' ') || 'untitled';
}
