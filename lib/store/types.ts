'use client';

import type { StateCreator } from 'zustand';
import { resolveTypes, type TypeDefinition } from '@/lib/scene/classification';
import type { AppState, Mutators } from './index';

/** Mock-only: custom types live in client state, no bridge call yet. */
export interface TypesSlice {
  customTypes: TypeDefinition[];
  addType(t: TypeDefinition): void;
  updateType(id: string, patch: Partial<TypeDefinition>): void;
  removeType(id: string): void;
  resolvedTypes(): TypeDefinition[];
}

export const createTypesSlice: StateCreator<AppState, Mutators, [], TypesSlice> = (set, get) => ({
  customTypes: [],
  addType: (t) => set((s) => ({ customTypes: [...s.customTypes, t] })),
  updateType: (id, patch) =>
    set((s) => ({
      customTypes: s.customTypes.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    })),
  removeType: (id) => set((s) => ({ customTypes: s.customTypes.filter((t) => t.id !== id) })),
  resolvedTypes: () => resolveTypes(get().customTypes),
});
