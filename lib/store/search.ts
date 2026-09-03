import type { StateCreator } from 'zustand';
import { getBridge, type SearchHit } from '@/lib/bridge';
import type { AppState, Mutators } from './index';

/**
 * matchedIds is null when no search is active — nothing on canvas dims. Once
 * a query lands hits, it holds the matched entry ids (even empty), so a
 * zero-result search reads as "nothing here" on the canvas too.
 */
export interface SearchSlice {
  searchQuery: string;
  searchHits: SearchHit[];
  matchedIds: Set<string> | null;
  /** Edges with BOTH ends in the result set — the subgraph the query induces. */
  matchedEdgeIds: Set<string> | null;
  /** Hits that no matched edge touches: found, but standing alone in this result. */
  orphanIds: Set<string> | null;

  runSearch(query: string): Promise<void>;
  clearSearch(): void;
}

export const createSearchSlice: StateCreator<AppState, Mutators, [], SearchSlice> = (set, get) => ({
  searchQuery: '',
  searchHits: [],
  matchedIds: null,
  matchedEdgeIds: null,
  orphanIds: null,

  async runSearch(query) {
    set({ searchQuery: query });
    if (query.trim().length < 2) {
      set({ searchHits: [], matchedIds: null, matchedEdgeIds: null, orphanIds: null });
      return;
    }
    const hits = await getBridge().searchEntries(query);
    // Stale-response guard: a later keystroke may have already moved on.
    if (get().searchQuery !== query) return;
    const matchedIds = new Set(hits.map((h) => h.entryId));

    // A search result is a subgraph, not a list: keep the edges whose two ends
    // are both hits, and note which hits none of them reach.
    const matchedEdgeIds = new Set<string>();
    const linked = new Set<string>();
    for (const e of get().edges) {
      if (e.status === 'dismissed') continue;
      if (!matchedIds.has(e.entryA) || !matchedIds.has(e.entryB)) continue;
      matchedEdgeIds.add(e.id);
      linked.add(e.entryA);
      linked.add(e.entryB);
    }
    const orphanIds = new Set([...matchedIds].filter((id) => !linked.has(id)));

    set({ searchHits: hits, matchedIds, matchedEdgeIds, orphanIds });
  },

  clearSearch() {
    set({ searchQuery: '', searchHits: [], matchedIds: null, matchedEdgeIds: null, orphanIds: null });
  },
});
