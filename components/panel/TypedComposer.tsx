'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import styles from './TypedComposer.module.css';

/** Words per minute, to give a typed entry a duration so it can be sized. */
const WPM = 150;

/**
 * §4 — the typed path. Nobody dictates a list, so there has to be a keyboard
 * way in. No audio, no fingerprint, no transcription: the text *is* the record.
 */
export default function TypedComposer({ onClose }: { onClose(): void }) {
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);
  const upsertEntry = useApp((s) => s.upsertEntry);
  const openEntry = useApp((s) => s.openEntry);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  const save = async () => {
    const body = text.trim();
    if (!body || saving) return;
    setSaving(true);
    const words = body.split(/\s+/).length;
    const entry = await getBridge().createEntry({
      transcript: body,
      durationMs: Math.round((words / WPM) * 60_000),
      fingerprint: [],
      typed: true,
    });
    upsertEntry(entry);
    openEntry(entry.id);
    onClose();
  };

  return (
    <div className={styles.composer}>
      <div className={styles.head}>
        <span className={styles.label}>typed note</span>
        <span className={styles.hint}>no audio, no question — the text is the record</span>
      </div>

      <textarea
        ref={ref}
        className={`${styles.field} selectable`}
        value={text}
        rows={3}
        placeholder="a list, a reminder, something you would not say out loud"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            onClose();
            return;
          }
          // Enter saves; Shift+Enter is a newline, since lists are the point.
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            void save();
          }
        }}
      />

      <div className={styles.foot}>
        <span className={styles.hint}>
          <kbd className={styles.kbd}>Enter</kbd> saves ·{' '}
          <kbd className={styles.kbd}>Shift</kbd>+<kbd className={styles.kbd}>Enter</kbd> new line
        </span>
        <div className={styles.actions}>
          <button type="button" className={styles.cancel} onClick={onClose}>
            esc
          </button>
          <button
            type="button"
            className={styles.save}
            onClick={() => void save()}
            disabled={!text.trim() || saving}
          >
            Keep
          </button>
        </div>
      </div>
    </div>
  );
}
