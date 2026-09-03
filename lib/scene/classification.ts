/**
 * Classification registry (§3.6). Stored types are unbounded; the letterform
 * vocabulary is fixed at 3 + inert. A user type binds to a slot for its
 * letterform and carries its own mark glyph.
 */

import type { Entry, ProbeTier, RenderedType } from '@/lib/types';

export type SlotId = RenderedType;
export type EdgeTreatment = 'crisp' | 'irregular' | 'soft' | 'plain';

/** Built-in marks are drawn paths, not characters — pixel-exact at 11px and
 *  independent of font availability. User marks are a single grapheme. */
export type Mark =
  | { kind: 'glyph'; id: SlotId }
  | { kind: 'char'; char: string };

export interface RenderSlot {
  id: SlotId;
  edge: EdgeTreatment;
  family: 'serif' | 'mono';
  weight: 300 | 400 | 500 | 600;
  italic: boolean;
  tracking: number;
  opacity: number;
}

export const SLOTS: Record<SlotId, RenderSlot> = {
  claim: { id: 'claim', edge: 'crisp', family: 'serif', weight: 600, italic: false, tracking: 0, opacity: 1 },
  rant: { id: 'rant', edge: 'irregular', family: 'serif', weight: 400, italic: true, tracking: 0, opacity: 1 },
  felt: { id: 'felt', edge: 'soft', family: 'serif', weight: 300, italic: false, tracking: 0.6, opacity: 0.8 },
  inert: { id: 'inert', edge: 'plain', family: 'mono', weight: 400, italic: false, tracking: -0.2, opacity: 0.7 },
};

/** Offered in the type editor so the common case needs no hunting. Each one
 *  passes the validator; none is a disc. */
export const PRESET_MARKS = ['†', '‡', '¶', '§', '△', '◇', '⊹', '∴', '⟡', '⌖'];

/** Nothing else in the app may be a filled circle — that is the open question. */
export const RESERVED_SHAPE = 'filled-circle';

export interface TypeDefinition {
  id: string;
  label: string;
  builtIn: boolean;
  match: string;
  prompt: string | null;
  tier: ProbeTier;
  /** Letterform binding. null = default letterform, no mark. */
  renderSlot: SlotId | null;
  /** null on built-ins: they use their slot's drawn glyph. */
  mark: Mark | null;
  /** §3.6 rule 1 — must be set deliberately for a user type to fire on its own. */
  autoApproved: boolean;
}

export const BUILT_IN_TYPES: TypeDefinition[] = [
  { id: 'claim', label: 'claim', builtIn: true, match: 'a stated position with reasons', prompt: null, tier: 'safe', renderSlot: 'claim', mark: null, autoApproved: true },
  { id: 'rant', label: 'rant', builtIn: true, match: 'working something out, unresolved', prompt: null, tier: 'safe', renderSlot: 'rant', mark: null, autoApproved: true },
  { id: 'felt', label: 'felt', builtIn: true, match: 'personal, emotionally live', prompt: null, tier: 'silent', renderSlot: 'felt', mark: null, autoApproved: true },
  { id: 'inert', label: 'inert', builtIn: true, match: 'lists, admin, reference, intent notes', prompt: null, tier: 'silent', renderSlot: 'inert', mark: null, autoApproved: true },
];

const BUILT_IN_IDS = new Set(BUILT_IN_TYPES.map((t) => t.id));
const AUTO_FIRING: ProbeTier[] = ['safe', 'retrieval'];

/**
 * `custom` is user data, so nothing in it is trusted for a safety decision:
 * builtIn is derived from the id, and any auto-firing tier is demoted to heavy
 * unless the user opted in explicitly.
 */
export function resolveTypes(custom: TypeDefinition[] = []): TypeDefinition[] {
  const merged = new Map(BUILT_IN_TYPES.map((t) => [t.id, t]));
  for (const t of custom) {
    if (BUILT_IN_IDS.has(t.id)) continue;
    const optedIn = t.autoApproved === true;
    const tier: ProbeTier = !optedIn && AUTO_FIRING.includes(t.tier) ? 'heavy' : t.tier;
    const slot = t.renderSlot && SLOTS[t.renderSlot] ? t.renderSlot : null;
    merged.set(t.id, { ...t, builtIn: false, tier, renderSlot: slot, autoApproved: optedIn });
  }
  return [...merged.values()];
}

function definitionFor(entry: Entry, types: TypeDefinition[]): TypeDefinition | undefined {
  return types.find((t) => t.id === entry.storedType);
}

export function slotFor(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): RenderSlot | null {
  const def = definitionFor(entry, types);
  const id = def ? def.renderSlot : entry.type;
  return id ? SLOTS[id] ?? null : null;
}

/** A user type's own mark wins; otherwise the slot's drawn glyph. */
export function markFor(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): Mark | null {
  const def = definitionFor(entry, types);
  if (def?.mark) return def.mark;
  const slot = slotFor(entry, types);
  return slot ? { kind: 'glyph', id: slot.id } : null;
}

export interface LegendRow {
  label: string;
  gloss: string;
  mark: Mark | null;
  slot: RenderSlot | null;
  builtIn: boolean;
}

export function legend(types: TypeDefinition[] = BUILT_IN_TYPES): LegendRow[] {
  return types
    .filter((t) => t.renderSlot || t.mark)
    .map((t) => ({
      label: t.label,
      gloss: t.match,
      mark: t.mark ?? (t.renderSlot ? { kind: 'glyph' as const, id: t.renderSlot } : null),
      slot: t.renderSlot ? SLOTS[t.renderSlot] : null,
      builtIn: t.builtIn,
    }));
}

/** §3.6 rule 2 — suppression is not overridable by a user-defined type. */
export function mayProbe(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): boolean {
  if (entry.type === 'felt' || entry.type === 'inert') return false;
  if (entry.durationMs < 30_000) return false;
  const def = definitionFor(entry, types);
  return def ? def.tier !== 'silent' : true;
}

/**
 * §3.1 modes D/E/F — the Heavy tier, invoked only. A/B/C already fire as the
 * automatic question and G arrives as a proposed edge, so neither belongs here.
 */
export interface Probe {
  id: 'steelman' | 'boundary' | 'disconfirming' | 'munchhausen' | 'feynman';
  label: string;
  hint: string;
}

const ALL_PROBES: Probe[] = [
  { id: 'steelman', label: 'steelman it', hint: 'state it better than you did, then push' },
  { id: 'boundary', label: 'find the edge', hint: 'where does this stop holding?' },
  { id: 'disconfirming', label: 'what would break it', hint: 'what would make you drop this?' },
  { id: 'munchhausen', label: 'ask why, four times', hint: 'follow the reasons until they bottom out' },
  { id: 'feynman', label: 'explain it simply', hint: 'only useful where there is a concept to master' },
];

/** Reference-ish material — the only place Feynman is not grotesque (§3.1 F). */
const CONCEPT_TYPES = new Set(['reference', 'definition', 'quote', 'source', 'excerpt']);

/**
 * Which probes an entry may be asked. Empty for felt, inert and sub-30s — the
 * suppression list, and it is not overridable (§3.2, §3.6).
 */
export function invokedProbes(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): Probe[] {
  if (!mayProbe(entry, types)) return [];
  const slot = slotFor(entry, types);
  return ALL_PROBES.filter((p) => {
    // §3.3 — Münchhausen needs a claim or a rant, never a list or a source.
    if (p.id === 'munchhausen') return slot?.id === 'claim' || slot?.id === 'rant';
    if (p.id === 'feynman') return CONCEPT_TYPES.has(entry.storedType);
    return true;
  });
}

export function edgeTreatmentFor(entry: Entry, types?: TypeDefinition[]): EdgeTreatment {
  return slotFor(entry, types)?.edge ?? 'plain';
}

export type { RenderedType };
