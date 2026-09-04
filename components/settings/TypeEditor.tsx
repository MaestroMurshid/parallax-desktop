'use client';

import { useMemo, useState } from 'react';
import MarkGlyph from '@/components/canvas/MarkGlyph';
import {
  PRESET_MARKS,
  SLOTS,
  resolveTypes,
  type SlotId,
  type TypeDefinition,
} from '@/lib/scene/classification';
import { REJECT_MESSAGE, validateMark } from '@/lib/scene/mark-validator';
import { useApp } from '@/lib/store';
import type { ProbeTier } from '@/lib/types';
import styles from './TypeEditor.module.css';

/** What a user may pick. `retrieval` describes mode G, which is edge-driven
 *  and not a property of a type, so it is not offered. */
const TIERS: ProbeTier[] = ['silent', 'heavy', 'safe'];

/** The tier word is the gate; on its own it tells the user nothing. */
const TIER_TEXT: Record<ProbeTier, string> = {
  safe: 'asks on its own',
  silent: 'never asks on its own',
  heavy: 'only when you ask',
  retrieval: 'pairs it up, never asks',
};

/**
 * Tier alone stopped describing behaviour once Feynman was ungated from
 * position: evidence and note are both silent, but evidence can still be asked
 * on a selection and note reaches nothing. Read the role for built-ins.
 */
function tierGloss(t: TypeDefinition): string {
  if (t.builtIn) {
    if (t.role === 'position') return 'asks on its own';
    if (t.role === 'evidence') return 'asks you to show you have it';
    return 'never asks';
  }
  const base = TIER_TEXT[t.tier];
  return base;
}
const SLOT_IDS = Object.keys(SLOTS) as SlotId[];
const slug = (s: string) => s.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');

const blank = {
  label: '',
  match: '',
  prompt: '',
  tier: 'heavy' as ProbeTier,
  role: '' as SlotId | '',
  mark: '',
  autoApproved: true,
};

export default function TypeEditor() {
  const customTypes = useApp((s) => s.customTypes);
  const addType = useApp((s) => s.addType);
  const removeType = useApp((s) => s.removeType);
  const [draft, setDraft] = useState(blank);

  const resolved = useMemo(() => resolveTypes(customTypes), [customTypes]);
  const markResult = useMemo(() => (draft.mark ? validateMark(draft.mark) : null), [draft.mark]);
  const slot = draft.role ? SLOTS[draft.role] : null;
  const canSubmit = draft.label.trim() !== '' && markResult?.ok === true;

  function submit() {
    if (!canSubmit || !markResult?.ok) return;
    const id = slug(draft.label);
    if (!id || resolved.some((t) => t.id === id)) return;
    addType({
      id,
      label: draft.label.trim(),
      builtIn: false,
      match: draft.match.trim() || 'manual',
      prompt: draft.prompt.trim() || null,
      tier: draft.tier,
      role: draft.role || null,
      mark: { kind: 'char', char: markResult.char },
      autoApproved: draft.autoApproved,
    });
    setDraft(blank);
  }

  return (
    <>
      <div className={styles.list}>
        {resolved.map((t) => (
          <div key={t.id} className={styles.typeRow}>
            <MarkGlyph mark={markMarkOf(t)} size={12} />
            <span className={styles.typeName}>{t.label}</span>
            <span className={styles.typeGloss}>{t.match}</span>
            <span className={styles.typeMeta}>
              {tierGloss(t)}
              {!t.builtIn && (
                <button type="button" className={styles.remove} onClick={() => removeType(t.id)}>
                  {' '}
                  remove
                </button>
              )}
            </span>
          </div>
        ))}
      </div>

      <div className={styles.form}>
        <div className={styles.field}>
          <span className={styles.fieldLabel}>label</span>
          <input
            className={styles.input}
            value={draft.label}
            onChange={(e) => setDraft({ ...draft, label: e.target.value })}
            placeholder="self-improvement"
          />
          <span className={styles.hint}>What it is called, on the canvas and in this list.</span>
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>match</span>
          <input
            className={styles.input}
            value={draft.match}
            onChange={(e) => setDraft({ ...draft, match: e.target.value })}
            placeholder="manual, or how the classifier should recognise it"
          />
          <span className={styles.hint}>How an entry gets this type. Describe it in your own words &mdash; this sentence is what the model matches against. Write &ldquo;manual&rdquo; to tag entries yourself instead.</span>
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>prompt</span>
          <textarea
            className={styles.textarea}
            value={draft.prompt}
            onChange={(e) => setDraft({ ...draft, prompt: e.target.value })}
            placeholder="help me move toward the goal in this entry"
          />
          <span className={styles.hint}>What it should ask when it fires. Left empty, it asks what it would have asked anyway.</span>
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>letterform</span>
          <select
            className={styles.select}
            value={draft.role}
            onChange={(e) => setDraft({ ...draft, role: e.target.value as SlotId | '' })}
          >
            <option value="">none — default letterform</option>
            {SLOT_IDS.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
          <span className={styles.hint}>
            Which of the three the canvas should draw it like. It borrows the look, not the
            behaviour &mdash; the tier below decides that.
          </span>
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>mark</span>
          <div className={styles.chips}>
            {PRESET_MARKS.map((m) => (
              <button
                key={m}
                type="button"
                className={draft.mark === m ? styles.chipOn : styles.chip}
                onClick={() => setDraft({ ...draft, mark: m })}
              >
                {m}
              </button>
            ))}
          </div>
          <div className={styles.markRow}>
            <input
              className={`${styles.input} ${styles.markInput}`}
              value={draft.mark}
              onChange={(e) => setDraft({ ...draft, mark: e.target.value })}
              placeholder="paste"
            />
            <span className={styles.fieldLabel}>or paste any character</span>
          </div>
          <span className={styles.hint}>
            The glyph on the rail, so you can pick this type out at a glance. Anything too heavy,
            unavailable in the font, or shaped like the open-question dot is refused.
          </span>
        </div>

        {markResult?.ok && (
          <div className={styles.preview}>
            <MarkGlyph mark={{ kind: 'char', char: markResult.char }} />
            <span
              className={styles.previewTitle}
              style={
                slot
                  ? {
                      fontFamily: slot.family === 'mono' ? 'var(--font-mono)' : 'var(--font-serif)',
                      fontWeight: slot.weight,
                      fontStyle: slot.italic ? 'italic' : 'normal',
                      letterSpacing: `${slot.tracking}px`,
                      opacity: slot.opacity,
                    }
                  : undefined
              }
            >
              {draft.label.trim() || 'half-formed thinking'}
            </span>
          </div>
        )}

        {markResult && !markResult.ok && (
          <p className={styles.reject}>{REJECT_MESSAGE[markResult.reason]}</p>
        )}

        <div className={styles.field}>
          <span className={styles.fieldLabel}>tier</span>
          <select
            className={styles.select}
            value={draft.tier}
            onChange={(e) => setDraft({ ...draft, tier: e.target.value as ProbeTier })}
          >
            {TIERS.map((t) => (
              <option key={t} value={t}>
                {t} &mdash; {TIER_TEXT[t]}
              </option>
            ))}
          </select>
          <span className={styles.hint}>
            How far it may go. Whatever you pick, it never fires on a note, on someone
            else&rsquo;s words, or on an entry that reads as live.
          </span>
        </div>

        <button type="button" className={styles.submit} disabled={!canSubmit} onClick={submit}>
          add type
        </button>
      </div>
    </>
  );
}

/** Legend and rows want the mark for a definition, not for an entry. */
function markMarkOf(t: TypeDefinition) {
  if (t.mark) return t.mark;
  return t.role ? ({ kind: 'glyph', id: t.role } as const) : null;
}
