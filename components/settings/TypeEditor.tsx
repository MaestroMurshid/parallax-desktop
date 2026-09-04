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

const TIERS: ProbeTier[] = ['silent', 'heavy', 'safe', 'retrieval'];

/** The tier word is the gate; on its own it tells the user nothing. */
const TIER_TEXT: Record<ProbeTier, string> = {
  safe: 'asks on its own',
  silent: 'never asks on its own',
  heavy: 'only when you ask',
  retrieval: 'pairs it up, never asks',
};

function tierGloss(t: TypeDefinition): string {
  const base = TIER_TEXT[t.tier];
  return t.builtIn || t.autoApproved ? base : `${base} · not approved to fire`;
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
  autoApproved: false,
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
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>match</span>
          <input
            className={styles.input}
            value={draft.match}
            onChange={(e) => setDraft({ ...draft, match: e.target.value })}
            placeholder="manual, or how the classifier should recognise it"
          />
        </div>

        <div className={styles.field}>
          <span className={styles.fieldLabel}>prompt</span>
          <textarea
            className={styles.textarea}
            value={draft.prompt}
            onChange={(e) => setDraft({ ...draft, prompt: e.target.value })}
            placeholder="help me move toward the goal in this entry"
          />
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
                {t}
              </option>
            ))}
          </select>
        </div>

        <label className={styles.markRow}>
          <input
            type="checkbox"
            checked={draft.autoApproved}
            onChange={(e) => setDraft({ ...draft, autoApproved: e.target.checked })}
          />
          <span className={styles.fieldLabel}>let this fire automatically</span>
        </label>

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
