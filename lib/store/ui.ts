import type { StateCreator } from 'zustand';
import type { AppState, Mutators } from './index';

/** §9.1 — one page with panels, not routes. Overlays are client state. */
export type Overlay = 'none' | 'entry' | 'tasks' | 'settings' | 'onboarding';
export type Theme = 'system' | 'light' | 'dark';

export interface UiSlice {
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

  openEntry(id: string): void;
  closeOverlay(): void;
  setOverlay(o: Overlay): void;
  setHovered(id: string | null): void;
  toggleAnalysis(): void;
  setSampleLoaded(v: boolean): void;
}

export const createUiSlice: StateCreator<AppState, Mutators, [], UiSlice> = (set) => ({
  overlay: 'none',
  theme: 'system',
  setTheme: (theme) => set({ theme }),
  selectedEntryId: null,
  hoveredEntryId: null,
  analysisOpen: false,
  sampleLoaded: false,
  dragging: null,
  composing: false,
  connecting: null,
  connectSource: null,
  pendingLink: null,

  openEntry(id) {
    set({ overlay: 'entry', selectedEntryId: id, analysisOpen: false });
  },
  closeOverlay() {
    set({ overlay: 'none', selectedEntryId: null, analysisOpen: false });
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
