import type { StateCreator } from 'zustand';
import type { ActionItem, Edge, Entry, Question } from '@/lib/types';
import { getBridge, type CorpusImport, type ImportMode } from '@/lib/bridge';
import { titleBox } from '@/lib/scene/lexicon';
import { relaxLayout } from '@/lib/scene/relax';
import type { AppState, Mutators } from './index';

export interface CorpusSlice {
  entries: Map<string, Entry>;
  /** Insertion order = chronological. Placement depends on it (§5.1). */
  order: string[];
  edges: Edge[];
  /** Questions accumulate. Overwriting was the whole thing this app exists
   *  to stop: the interrogation detaching from the note and going missing. */
  questions: Map<string, Question[]>;
  actionItems: ActionItem[];
  loaded: boolean;

  loadCorpus(): Promise<void>;
  upsertEntry(entry: Entry): void;
  /** Re-reads one entry after enrichment wrote to it. Fetches rather than
   *  recomputes: classification and the question are already on disk. */
  refreshEntry(id: string): Promise<void>;
  /** Appends. A question asked of an entry stays on it (§3.4). */
  addQuestion(entryId: string, question: Question): void;
  /** Marks one question answered without disturbing the others. */
  markAnswered(entryId: string, questionId: string): void;
  /** Strikes one out. Kept in the record — the signal is worth more than the tidiness. */
  dismissQuestion(entryId: string, questionId: string): Promise<void>;
  /** Commits a drag (§5.1). Called once on release, never during the drag. */
  moveEntry(id: string, x: number, y: number): Promise<void>;
  /**
   * Tidy the field: shorten the drawn lines without letting two titles collide
   * (§5.1 forbids the app re-solving on its own — this only ever runs because
   * you asked). Positions are committed like any drag, so it is undoable by
   * dragging and it survives a reload.
   */
  relayout(): Promise<void>;
  deleteEntry(id: string): Promise<void>;
  /**
   * Fixes a mis-transcription. The only edit a note takes — it is the verbatim
   * record of what was said, so there is no append and no rewrite.
   *
   * Takes the entry the bridge returns rather than patching the transcript
   * locally: the correction re-anchors spans and drops the ones whose words are
   * gone, and none of that is derivable here.
   */
  correctTranscript(id: string, transcript: string): Promise<void>;
  /** §6.3 — user-declared only, and the text is the point, not the flag. */
  resolveEntry(id: string, text: string): Promise<void>;
  reopenEntry(id: string): Promise<void>;
  /** §5.4 — naming the relation is the step that carries the benefit. */
  linkEntries(a: string, b: string, relation: Edge['relation']): Promise<void>;
  dismissEdge(id: string): Promise<void>;
  acceptEdge(id: string): Promise<void>;
  toggleActionItem(id: string): Promise<void>;
  loadSample(): Promise<void>;
  /** Removes the sample and nothing else — the pair to loadSample in settings. */
  clearSample(): Promise<void>;
  /** Wipes the corpus, sample or not. What the status bar's `clear` means. */
  clearAll(): Promise<void>;
  importCorpus(data: CorpusImport, mode: ImportMode): Promise<void>;
  /** Applies the upload the bridge already read, then reloads. */
  applyUpload(mode: ImportMode): Promise<void>;

  // -- derived, memo-free because they are O(edges) over a few hundred items --
  /** Layers behind an entry's rings (§6.2). Children never get their own blob. */
  returnsFor(entryId: string): number;
  edgesFor(entryId: string): Edge[];
  /** Entries with no drawn edge — dimmed fill, the honest case (§5.3). */
  isIsolated(entryId: string): boolean;
}

const drawn = (e: Edge) => e.status !== 'dismissed';

export const createCorpusSlice: StateCreator<AppState, Mutators, [], CorpusSlice> = (set, get) => ({
  entries: new Map(),
  order: [],
  edges: [],
  questions: new Map(),
  actionItems: [],
  loaded: false,

  async loadCorpus() {
    const bridge = getBridge();
    const [entries, edges, actionItems] = await Promise.all([
      bridge.listEntries(),
      bridge.listEdges(),
      bridge.listActionItems(),
    ]);
    const map = new Map(entries.map((e) => [e.id, e]));
    const order = [...entries]
      .sort((a, b) => a.createdAt.localeCompare(b.createdAt))
      .map((e) => e.id);

    // The whole history, not the one still open: getQuestion returns the oldest
    // unanswered question, so a reload built on it silently dropped every
    // question that had been answered or dismissed — out of the entry and out
    // of the export with it. One query for the corpus, not one per entry.
    const questions = new Map<string, Question[]>();
    const all = await bridge.listQuestions?.();
    if (all) {
      for (const q of all) {
        if (!map.has(q.entryId)) continue;
        const prior = questions.get(q.entryId);
        if (prior) prior.push(q);
        else questions.set(q.entryId, [q]);
      }
    } else {
      await Promise.all(
        entries.map(async (e) => {
          const q = await bridge.getQuestion(e.id);
          if (q) questions.set(e.id, [q]);
        }),
      );
    }

    set({ entries: map, order, edges, actionItems, questions, loaded: true });
  },

  async refreshEntry(id) {
    const bridge = getBridge();
    // Edges come back too. The pass that classifies a note also proposes its
    // connections, and reading only the entry meant every connection the app
    // found sat in the database until the next full load -- so the one mechanic
    // worth watching happen was the one that never appeared while you watched.
    // Tasks too, or a note's new tasks wait for the next launch.
    const [entry, question, edges, actionItems] = await Promise.all([
      bridge.getEntry(id),
      bridge.getQuestion(id),
      bridge.listEdges(),
      bridge.listActionItems(),
    ]);
    // Deleted while enrichment was running.
    if (!entry) return;
    get().upsertEntry(entry);
    set({ edges, actionItems });

    // Merged by id rather than replaced. Overwriting with `[question]` dropped
    // every earlier question on the entry — getQuestion returns only the open
    // one — and questions accumulate (§3.4); deduping by id is what keeps a
    // refresh from doubling one up.
    if (question) get().addQuestion(id, question);
  },

  upsertEntry(entry) {
    const entries = new Map(get().entries);
    const isNew = !entries.has(entry.id);
    entries.set(entry.id, entry);
    set({ entries, order: isNew ? [...get().order, entry.id] : get().order });
  },

  async dismissQuestion(entryId: string, questionId: string) {
    await getBridge().dismissQuestion(entryId, questionId);
    const questions = new Map(get().questions);
    const prior = questions.get(entryId);
    if (!prior) return;
    questions.set(entryId, prior.map((q) => (q.id === questionId ? { ...q, dismissed: true } : q)));
    set({ questions });
  },

  markAnswered(entryId: string, questionId: string) {
    const questions = new Map(get().questions);
    const prior = questions.get(entryId);
    if (!prior) return;
    questions.set(entryId, prior.map((q) => (q.id === questionId ? { ...q, answered: true } : q)));
    set({ questions });
  },

  addQuestion(entryId: string, question: Question) {
    const questions = new Map(get().questions);
    const prior = questions.get(entryId) ?? [];
    // Append, never replace. §3.4 bans a regenerate button for the same reason:
    // rerolling until the question is agreeable is the echo chamber by the back door.
    questions.set(entryId, [...prior.filter((q) => q.id !== question.id), question]);
    set({ questions });
  },

  async moveEntry(id, x, y) {
    const next = await getBridge().moveEntry(id, x, y);
    get().upsertEntry(next);
  },

  async relayout() {
    // Replies are canvas nodes too. Leaving them out made "tidy" move only
    // part of the visible field and reintroduced overlapping reply titles.
    const entries = [...get().entries.values()];
    if (entries.length < 2) return;

    // Only what the canvas draws pulls: dismissed edges are not on screen, so
    // shortening them would move notes for a line nobody can see.
    const links = new Map<string, string[]>();
    const link = (from: string, to: string) => {
      const existing = links.get(from);
      if (existing) existing.push(to);
      else links.set(from, [to]);
    };
    for (const edge of get().edges) {
      if (edge.status === 'dismissed') continue;
      link(edge.entryA, edge.entryB);
      link(edge.entryB, edge.entryA);
    }

    const moved = relaxLayout(
      entries.map((e) => {
        const box = titleBox(e);
        return { id: e.id, x: e.x, y: e.y, halfW: box.w / 2, halfH: box.h / 2 };
      }),
      links,
    );

    // Through moveEntry so each new position goes through the same path a drag
    // takes — the bridge is what owns a position, not the store.
    await Promise.all([...moved].map(([id, p]) => get().moveEntry(id, p.x, p.y)));
  },

  async deleteEntry(id) {
    await getBridge().deleteEntry(id);
    await get().loadCorpus();
  },

  async correctTranscript(id, transcript) {
    const bridge = getBridge();
    get().upsertEntry(await bridge.correctTranscript(id, transcript));

    // The same correction re-anchored every question on this entry, and a
    // question's span is what the panel quotes back. `listQuestions` is the only
    // call that returns the whole history — `getQuestion` gives the oldest open
    // one — so without it the answered and dismissed ones keep quoting the text
    // that was just fixed.
    const all = await bridge.listQuestions?.();
    const questions = new Map(get().questions);
    if (all) {
      questions.set(
        id,
        all.filter((q) => q.entryId === id),
      );
    } else {
      // No way to re-read them, so the spans in hand are the ones the old text
      // produced and every one of them now indexes the wrong words. Dropping
      // the anchor loses a highlight; keeping it quotes the user back a
      // sentence they never said, which is the worse of the two.
      questions.set(id, (questions.get(id) ?? []).map((q) => ({ ...q, span: null })));
    }
    set({ questions });
  },

  async resolveEntry(id, text) {
    get().upsertEntry(await getBridge().resolveEntry(id, text));
  },

  async reopenEntry(id) {
    get().upsertEntry(await getBridge().reopenEntry(id));
  },

  async linkEntries(a, b, relation) {
    const edge = await getBridge().createManualEdge(a, b, relation);
    set({ edges: [...get().edges, edge] });
  },

  async dismissEdge(id) {
    // Dismissals are training signal, not just UI (§6.1) — so they persist
    // through the bridge rather than being dropped from local state.
    await getBridge().dismissEdge(id);
    set({ edges: get().edges.map((e) => (e.id === id ? { ...e, status: 'dismissed' } : e)) });
  },

  async acceptEdge(id) {
    await getBridge().acceptEdge(id);
    set({ edges: get().edges.map((e) => (e.id === id ? { ...e, status: 'accepted' } : e)) });
  },

  async toggleActionItem(id) {
    const item = get().actionItems.find((a) => a.id === id);
    if (!item) return;
    await getBridge().setActionItemDone(id, !item.done);
    set({ actionItems: get().actionItems.map((a) => (a.id === id ? { ...a, done: !a.done } : a)) });
  },

  async loadSample() {
    await getBridge().loadSampleCorpus();
    await get().loadCorpus();
  },

  async importCorpus(data, mode) {
    await getBridge().importCorpus(data, mode);
    await get().loadCorpus();
  },

  async applyUpload(mode) {
    await getBridge().applyUpload(mode);
    await get().loadCorpus();
  },

  async clearSample() {
    await getBridge().clearSampleCorpus();
    await get().loadCorpus();
  },

  // Replacing with an empty corpus rather than a new bridge verb: 'replace' is
  // already defined as wipe-then-load, so this needs nothing of Rust that
  // importCorpus does not already owe it (§9.1).
  async clearAll() {
    await getBridge().importCorpus({ entries: [], edges: [], questions: [] }, 'replace');
    await get().loadCorpus();
  },

  returnsFor(entryId) {
    let n = 0;
    for (const e of get().entries.values()) if (e.parentEdge === entryId) n++;
    return n;
  },

  edgesFor(entryId) {
    return get().edges.filter((e) => drawn(e) && (e.entryA === entryId || e.entryB === entryId));
  },

  isIsolated(entryId) {
    return get().edgesFor(entryId).length === 0;
  },
});
