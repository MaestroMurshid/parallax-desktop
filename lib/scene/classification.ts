/**
 * Classification registry (§3.6), on three orthogonal facets rather than one
 * flat list.
 *
 * The old four — claim / rant / felt / inert — were cut on four different bases
 * (rhetorical form, epistemic state, affective register, a null class), so they
 * were never mutually exclusive: a heated unresolved argument about your job is
 * all three of the first at once and the model had to pick one arbitrarily.
 * Worse, picking `felt` to stay safe also removed the entry from retrieval, so
 * protecting it cost you what it meant.
 *
 *   role       what the entry does      → letterform, retrieval
 *   register   is this emotionally live → the automatic question only
 *   provenance whose words are these    → what a question may anchor to (§7.3)
 *
 * Only `role` is rendered as a letterform. Register composes on top of it as a
 * treatment, and provenance lives on spans, so the canvas still carries three
 * channels — which is what §5.3's budget actually says.
 */

import type { Entry, ProbeTier, Register, Role, Span } from '@/lib/types';

export type SlotId = Role;
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

/**
 * The weight ladder carries role; the italic that used to mean `rant` retires
 * with it, because the rings say unresolved better than a slope does.
 */
export const SLOTS: Record<SlotId, RenderSlot> = {
  position: { id: 'position', edge: 'crisp', family: 'serif', weight: 600, italic: false, tracking: 0, opacity: 1 },
  evidence: { id: 'evidence', edge: 'plain', family: 'serif', weight: 400, italic: false, tracking: 0, opacity: 1 },
  note: { id: 'note', edge: 'plain', family: 'mono', weight: 400, italic: false, tracking: -0.2, opacity: 0.7 },
};

/**
 * Register is a treatment, not a slot. `live` reads as the soft, tracked-out
 * setting the old `felt` type had — but an entry keeps its role underneath, so
 * a live position is still a position everywhere retrieval looks.
 */
export function applyRegister(slot: RenderSlot, register: Register): RenderSlot {
  if (register !== 'live') return slot;
  return { ...slot, edge: 'soft', tracking: slot.tracking + 0.6, opacity: slot.opacity * 0.8 };
}

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
  /** Letterform binding — facet 1 only. null = default letterform, no mark. */
  role: SlotId | null;
  /** null on built-ins: they use their role's drawn glyph. */
  mark: Mark | null;
  /** §3.6 rule 1 — must be set deliberately for a user type to fire on its own. */
  autoApproved: boolean;
}

export const BUILT_IN_TYPES: TypeDefinition[] = [
  { id: 'position', label: 'position', builtIn: true, match: 'your own reasoning, asserted with grounds', prompt: null, tier: 'safe', role: 'position', mark: null, autoApproved: true },
  { id: 'evidence', label: 'evidence', builtIn: true, match: 'a fact, a number, a thing you noticed, or something you are learning', prompt: null, tier: 'silent', role: 'evidence', mark: null, autoApproved: true },
  { id: 'note', label: 'note', builtIn: true, match: 'admin, lists, intents, reminders', prompt: null, tier: 'silent', role: 'note', mark: null, autoApproved: true },
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
    // A type you defined fires like any built-in. §3.6 rule 1 wanted an extra
    // opt-in, but the suppressions below it are the ones with teeth: role,
    // provenance and register are not overridable, so the tier a user picks
    // can only ever be narrower than what those already allow.
    const role = t.role && SLOTS[t.role] ? t.role : null;
    merged.set(t.id, { ...t, builtIn: false, role, autoApproved: true });
  }
  return [...merged.values()];
}

function definitionFor(entry: Entry, types: TypeDefinition[]): TypeDefinition | undefined {
  return types.find((t) => t.id === entry.typeId);
}

/**
 * Facet 1 for gating. A user-defined type's role wins if it declares one,
 * otherwise the entry's own role — which is always set, so this never falls
 * through to "unknown" the way the old storedType lookup did.
 */
function roleOf(entry: Entry, types: TypeDefinition[]): Role {
  const def = definitionFor(entry, types);
  return def?.role ?? entry.role;
}

export function slotFor(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): RenderSlot | null {
  return SLOTS[roleOf(entry, types)] ?? null;
}

/** What the canvas draws: role letterform with register composed on top. */
export function treatmentFor(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): RenderSlot | null {
  const slot = slotFor(entry, types);
  return slot ? applyRegister(slot, entry.register) : null;
}

/** A user type's own mark wins; otherwise the role's drawn glyph. */
export function markFor(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): Mark | null {
  const def = definitionFor(entry, types);
  if (def?.mark) return def.mark;
  const slot = slotFor(entry, types);
  return slot ? { kind: 'glyph', id: slot.id } : null;
}

/**
 * What the entry panel shows. §3.6 says render the collapse, so this is the
 * legend's own vocabulary — never a raw registry key the legend never taught.
 */
export function typeLabel(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): string {
  const def = definitionFor(entry, types);
  const base = def?.label ?? roleOf(entry, types);
  return entry.register === 'live' ? `${base} · live` : base;
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
    .filter((t) => t.role || t.mark)
    .map((t) => ({
      label: t.label,
      gloss: t.match,
      mark: t.mark ?? (t.role ? { kind: 'glyph' as const, id: t.role } : null),
      slot: t.role ? SLOTS[t.role] : null,
      builtIn: t.builtIn,
    }));
}

/**
 * Facet 3. An entry offers something to push on unless every word of it is
 * someone else's. Spans mark attributed regions, so an entry with none is
 * wholly the user's own.
 */
export function hasOwnSpan(entry: Entry): boolean {
  const attributed = entry.spans.filter((s: Span) => s.attributed);
  if (attributed.length === 0) return true;
  const covered = attributed.reduce((n, s) => n + Math.max(0, s.end - s.start), 0);
  return covered < entry.transcript.length;
}

/**
 * The invoked path, adversarial half. §3.2: "the user chooses to be challenged,
 * so the risk of misfire is theirs" — so register does not gate here, and
 * neither does duration. Role and provenance still do, because there is nothing
 * to *push on* in a fact, a list, or a sentence that isn't yours.
 */
export function mayProbeOnRequest(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): boolean {
  if (!hasOwnSpan(entry)) return false;
  if (roleOf(entry, types) !== 'position') return false;
  const def = definitionFor(entry, types);
  // Silent means silent on both paths.
  return def ? def.tier !== 'silent' : true;
}

/**
 * Feynman is not a challenge. "Say it again without the word" takes no stance,
 * makes no claim and cannot misfire the way a steelman can, so it does not need
 * the position gate — and gating it there silently broke the case it exists
 * for. A note recording that something finally clicked classifies as
 * `evidence` far more often than as `position`, which meant the one move that
 * makes you do the thinking was the least reachable in the app.
 *
 * It needs a concept being held, which is anything that is not admin.
 */
export function mayAskToExplain(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): boolean {
  if (!hasOwnSpan(entry)) return false;
  return roleOf(entry, types) !== 'note';
}

/**
 * §3.1 modes D/E/F — the Heavy tier, invoked only. A/B/C already fire as the
 * automatic question and G arrives as a proposed edge, so neither belongs here.
 */
export interface Probe {
  id: 'steelman' | 'boundary' | 'disconfirming' | 'munchhausen' | 'feynman';
  label: string;
  hint: string;
  /**
   * §3.6's tier, declared rather than implied by position in the array. Safe
   * may fire on its own; Heavy is invoked only. Reading this off array order
   * is how a steelman ends up firing automatically.
   */
  tier: 'safe' | 'heavy';
}

const ALL_PROBES: Probe[] = [
  // §3.1 D — being understood before being challenged, but it states a position
  // of its own, so it is never the app's opening move.
  { id: 'steelman', label: 'steelman it', hint: 'state it better than you did, then push', tier: 'heavy' },
  // §3.1 B and C — the two that may open, per §3.2's A/B/C-or-nothing.
  { id: 'boundary', label: 'find the edge', hint: 'where does this stop holding?', tier: 'safe' },
  { id: 'disconfirming', label: 'what would break it', hint: 'what would make you drop this?', tier: 'safe' },
  // §3.1 E and F — §3.3 gates one, the other needs a concept being held.
  { id: 'munchhausen', label: 'ask why, four times', hint: 'follow the reasons until they bottom out', tier: 'heavy' },
  { id: 'feynman', label: 'explain it simply', hint: 'only useful where there is a concept to master', tier: 'heavy' },
];

/**
 * What the app may open with, unprompted — the primitive the automatic path is
 * built from. Three gates apply to everything: register, duration, and having
 * words of your own in it. Beyond that it depends on what the entry is.
 *
 * §3.2 restricts the opening move to A, B or C, and that holds for a position:
 * never a steelman, never Münchhausen. But it also said never F, and that was
 * wrong for the same reason F did not need the position gate — being asked to
 * say something back takes no stance and cannot wound. Withholding it until the
 * user thinks to ask means the one move that makes learning stick only fires
 * for someone who already knows to want it.
 */
export function automaticProbes(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): Probe[] {
  if (entry.register === 'live') return [];
  if (entry.durationMs < 30_000) return [];
  if (!hasOwnSpan(entry)) return [];

  const role = roleOf(entry, types);
  const def = definitionFor(entry, types);
  // A user type set to silent opens nothing, whatever its role.
  if (def && !def.builtIn && def.tier === 'silent') return [];

  if (role === 'position') {
    const mayInitiate = !def || def.builtIn || AUTO_FIRING.includes(def.tier);
    return mayInitiate ? ALL_PROBES.filter((p) => p.tier === 'safe') : [];
  }
  // A concept you are holding gets asked to be said back. Notes get nothing.
  if (role === 'evidence') return ALL_PROBES.filter((p) => p.id === 'feynman');
  return [];
}

/** Whether anything at all opens on its own. */
export function mayProbeAutomatically(
  entry: Entry,
  types: TypeDefinition[] = BUILT_IN_TYPES,
): boolean {
  return automaticProbes(entry, types).length > 0;
}

/**
 * Which probes an entry may be asked. Empty unless the entry is a position with
 * something of the user's own in it — the suppression is not overridable by a
 * user-defined type (§3.6 rule 2).
 */
export function invokedProbes(entry: Entry, types: TypeDefinition[] = BUILT_IN_TYPES): Probe[] {
  const canPush = mayProbeOnRequest(entry, types);
  const canExplain = mayAskToExplain(entry, types);
  if (!canPush && !canExplain) return [];
  return ALL_PROBES.filter((p) => (p.id === 'feynman' ? canExplain : canPush));
}

export function edgeTreatmentFor(entry: Entry, types?: TypeDefinition[]): EdgeTreatment {
  return treatmentFor(entry, types)?.edge ?? 'plain';
}

export type { Role };
