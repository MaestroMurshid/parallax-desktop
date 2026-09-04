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

  // -- derived, memo-free because they are O(edges) over a few hundred items --
  /** Layers behind an entry's rings (§6.2). Children never get their own blob. */
  returnsFor(entryId: string): number;
  edgesFor(entryId: string): Edge[];
  hasUnansweredQuestion(entryId: string): boolean;
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

    const questions = new Map<string, Question[]>();
    await Promise.all(
      entries.map(async (e) => {
        const q = await bridge.getQuestion(e.id);
        if (q) questions.set(e.id, [q]);
      }),
    );

    set({ entries: map, order, edges, actionItems, questions, loaded: true });
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
    const entries = [...get().entries.values()].filter((e) => e.parentEdge === null);
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

  hasUnansweredQuestion(entryId) {
    return (get().questions.get(entryId) ?? []).some((q) => !q.answered && !q.dismissed);
  },

  isIsolated(entryId) {
    return get().edgesFor(entryId).length === 0;
  },
});
