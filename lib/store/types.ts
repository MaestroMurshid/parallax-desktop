'use client';

import type { StateCreator } from 'zustand';
import { getBridge } from '@/lib/bridge';
import { resolveTypes, type TypeDefinition } from '@/lib/scene/classification';
import type { AppState, Mutators } from './index';

/** What `addType`/`updateType` send on: the fields a draft actually edits,
 *  never `id` or `builtIn` — the id is fixed at creation and a built-in's
 *  identity is not the user's to take over. */
type TypeEdits = Pick<TypeDefinition, 'label' | 'match' | 'prompt' | 'tier' | 'role' | 'mark'>;

export interface TypesSlice {
  customTypes: TypeDefinition[];
  /** False until `loadTypes` has read the corpus once — mirrors `loaded` on
   *  the corpus slice, so a settings screen opened before startup finishes
   *  reads as "still loading" rather than "no custom types exist". */
  typesLoaded: boolean;
  /** Reads built-ins and custom types from the one table Rust keeps them in
   *  (§3.6). Called once at startup, alongside `loadCorpus`. */
  loadTypes(): Promise<void>;
  /**
   * Optimistic: the type appears immediately, because the editor's own submit
   * button is the confirmation. Rolled back to what was there before if the
   * write fails — a duplicate id, a built-in id, or the retrieval tier a
   * custom type may not claim (Rust re-checks all three; user data is not
   * trusted for a safety decision even when the editor already filters it).
   */
  addType(id: string, edits: TypeEdits): Promise<void>;
  updateType(id: string, edits: TypeEdits): Promise<void>;
  /** Notes carrying this type fall back to their own role's built-in id —
   *  Rust does that in the same transaction as the delete, so the corpus is
   *  reloaded after rather than patched here, to pick up the type_id change. */
  removeType(id: string): Promise<void>;
  resolvedTypes(): TypeDefinition[];
}

export const createTypesSlice: StateCreator<AppState, Mutators, [], TypesSlice> = (set, get) => ({
  customTypes: [],
  typesLoaded: false,

  async loadTypes() {
    const all = await getBridge().listTypes();
    set({ customTypes: all.filter((t) => !t.builtIn), typesLoaded: true });
  },

  async addType(id, edits) {
    const optimistic: TypeDefinition = { id, builtIn: false, autoApproved: true, ...edits };
    set((s) => ({ customTypes: [...s.customTypes, optimistic] }));
    try {
      const created = await getBridge().createType({ id, ...edits });
      set((s) => ({ customTypes: s.customTypes.map((t) => (t.id === id ? created : t)) }));
    } catch (err) {
      set((s) => ({ customTypes: s.customTypes.filter((t) => t.id !== id) }));
      throw err;
    }
  },

  async updateType(id, edits) {
    const before = get().customTypes;
    set((s) => ({
      customTypes: s.customTypes.map((t) => (t.id === id ? { ...t, ...edits } : t)),
    }));
    try {
      const updated = await getBridge().updateType(id, edits);
      set((s) => ({ customTypes: s.customTypes.map((t) => (t.id === id ? updated : t)) }));
    } catch (err) {
      set({ customTypes: before });
      throw err;
    }
  },

  async removeType(id) {
    const before = get().customTypes;
    set((s) => ({ customTypes: s.customTypes.filter((t) => t.id !== id) }));
    try {
      await getBridge().deleteType(id);
      // The delete moved every carrying note's type_id to its own role, in
      // the same transaction on the Rust side — nothing here can derive that
      // without re-reading it.
      await get().loadCorpus();
    } catch (err) {
      set({ customTypes: before });
      throw err;
    }
  },

  resolvedTypes: () => resolveTypes(get().customTypes),
});
