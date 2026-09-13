import type { StateCreator } from 'zustand';
import type { AppState, Mutators } from './index';

/** §9.1 — one page with panels, not routes. Overlays are client state. */
export type Overlay = 'none' | 'entry' | 'tasks' | 'settings' | 'onboarding';
export type Theme = 'system' | 'light' | 'dark';
/**
 * The list is the way in. The graph is not intuitive on first contact, so it
 * stays available rather than primary — the premise is still a commonplace
 * book you think into, and a list is how anyone already reads one.
 */
export type View = 'list' | 'canvas';

export interface UiSlice {
  view: View;
  setView(v: View): void;
  /** Retrieval against dated transcripts. It recalls; it never writes (§8). */
  chatOpen: boolean;
  setChatOpen(v: boolean): void;
  /**
   * A question handed over from the search box, consumed once by the panel.
   *
   * Searching and asking were two inputs for what is, from the outside, one
   * act: you have a half-remembered thing and you want it back. Typing it twice
   * is the tax that made the second one not worth finding.
   */
  chatSeed: string | null;
  askAbout(query: string): void;
  clearChatSeed(): void;
  overlay: Overlay;
  theme: Theme;
  setTheme(t: Theme): void;
  selectedEntryId: string | null;
  hoveredEntryId: string | null;
  /**
   * §6.1 — collapsed by default; opening it first would colour how the user
   * reads their own words before they've read them straight.
   */
  analysisOpen: boolean;
  /** Sample corpus is offered from the empty state and always stays marked. */
  sampleLoaded: boolean;
  /**
   * Mirror of the setting of the same name, so the canvas and the list can
   * honour it without every component that draws a letterform being handed the
   * whole `Settings` object.
   *
   * A mirror and not the source: `page.tsx` writes it whenever settings load or
   * change, and nothing else does. Rust re-decides the same thing for the
   * questions it actually asks, so this only governs what is drawn.
   */
  liveRegister: boolean;
  setLiveRegister(v: boolean): void;
  /**
   * In-flight drag position (§5.1). Lives here rather than in the corpus so a
   * drag never rewrites entries 60 times a second; renderer and label overlay
   * subscribe transiently and the move is committed once on release.
   */
  dragging: { id: string; x: number; y: number } | null;
  /** Typed-note composer open (§4). */
  composing: boolean;
  setComposing(v: boolean): void;
  /** Drag-to-connect in flight (§5.4); committed only when a relation is named. */
  connecting: { fromId: string; x: number; y: number } | null;
  setConnecting(c: { fromId: string; x: number; y: number } | null): void;
  /** Linking from this entry, target not yet chosen (§5.4). The drag handle is
      one way in; this is the pointer-free one. */
  connectSource: string | null;
  setConnectSource(id: string | null): void;
  /** Both ends chosen, waiting on the relation word. */
  pendingLink: { fromId: string; toId: string } | null;
  setPendingLink(l: { fromId: string; toId: string } | null): void;
  setDragging(d: { id: string; x: number; y: number } | null): void;

  /**
   * Entries the model is working on right now (§9.4). Enrichment is the one
   * slow thing that happens without being asked for, and ~3.4s of silence
   * after a note lands reads as the app having decided not to bother.
   */
  enriching: ReadonlySet<string>;
  setEnriching(id: string, on: boolean): void;

  openEntry(id: string): void;
  closeOverlay(): void;
  setOverlay(o: Overlay): void;
  setHovered(id: string | null): void;
  toggleAnalysis(): void;
  setSampleLoaded(v: boolean): void;
}

export const createUiSlice: StateCreator<AppState, Mutators, [], UiSlice> = (set) => ({
  view: 'list',
  setView: (view) => set({ view }),
  chatOpen: false,
  setChatOpen: (chatOpen) => set({ chatOpen }),
  chatSeed: null,
  askAbout: (query) => set({ chatSeed: query, chatOpen: true }),
  clearChatSeed: () => set({ chatSeed: null }),
  overlay: 'none',
  theme: 'light',
  setTheme: (theme) => set({ theme }),
  selectedEntryId: null,
  hoveredEntryId: null,
  analysisOpen: true,
  sampleLoaded: false,
  // Matches the Rust default. A false here would show every note as neutral
  // for the moment before real settings arrive.
  liveRegister: true,
  enriching: new Set<string>(),
  setEnriching: (id, on) =>
    set((s) => {
      if (s.enriching.has(id) === on) return {};
      const next = new Set(s.enriching);
      if (on) next.add(id);
      else next.delete(id);
      return { enriching: next };
    }),
  dragging: null,
  composing: false,
  connecting: null,
  connectSource: null,
  pendingLink: null,

  openEntry(id) {
    set({ overlay: 'entry', selectedEntryId: id, analysisOpen: true });
  },
  closeOverlay() {
    set({ overlay: 'none', selectedEntryId: null, analysisOpen: true });
  },
  setOverlay(o) {
    set({ overlay: o });
  },
  setHovered(id) {
    set({ hoveredEntryId: id });
  },
  toggleAnalysis() {
    set((s) => ({ analysisOpen: !s.analysisOpen }));
  },
  setSampleLoaded(v) {
    set({ sampleLoaded: v });
  },
  setLiveRegister(v) {
    set({ liveRegister: v });
  },
  setComposing(v) {
    set({ composing: v });
  },
  setConnecting(c) {
    set({ connecting: c });
  },
  setConnectSource(id) {
    set({ connectSource: id });
  },
  setPendingLink(l) {
    set({ pendingLink: l, connectSource: null });
  },
  setDragging(d) {
    set({ dragging: d });
  },
});
