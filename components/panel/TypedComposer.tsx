'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { handOff, showPanel } from '@/lib/shell';
import { useApp } from '@/lib/store';
import styles from './TypedComposer.module.css';

/** Words per minute, to give a typed entry a duration so it can be sized. */
const WPM = 150;

/**
 * Dispatched on `window` when the capture hotkey lands while the composer is
 * already open. The composer decides what it means, because only it knows
 * whether there are words in it: empty, the key puts it away again; with
 * something written, it brings the field back rather than costing the draft.
 */
export const COMPOSER_HOTKEY = 'parallax:composer-hotkey';

/**
 * §4 — the typed path, and since text became the default, the main one. The
 * capture hotkey opens this wherever you are: in the canvas it sits where the
 * recorder sits; anywhere else it comes up in the floating panel window and
 * takes the keyboard, because a text field you cannot type into is not one.
 * No audio, no fingerprint, no transcription: the text *is* the record.
 */
export default function TypedComposer({
  onClose,
  inPanel = false,
}: {
  onClose(): void;
  /** Rendered in the floating panel window rather than over the canvas. */
  inPanel?: boolean;
}) {
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  // Read from the listeners below, which outlive any one render's `text`.
  const textRef = useRef('');
  textRef.current = text;
  const upsertEntry = useApp((s) => s.upsertEntry);
  const openEntry = useApp((s) => s.openEntry);
  const answering = useApp((s) => s.composeAnswering);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  useEffect(() => {
    const onHotkey = () => {
      if (!textRef.current.trim()) {
        onClose();
        return;
      }
      // Pressed again from another app with a draft still here: bring it back.
      if (inPanel) void showPanel(true);
      ref.current?.focus();
    };
    window.addEventListener(COMPOSER_HOTKEY, onHotkey);
    return () => window.removeEventListener(COMPOSER_HOTKEY, onHotkey);
  }, [onClose, inPanel]);

  // The panel window is given focus a beat after it is shown, which is after
  // this mounted and asked for it -- so the field asks again when it arrives.
  // Clicking away from an empty one is the same as escape; one with words in
  // it stays up, since a draft is never something to lose to a stray click.
  useEffect(() => {
    if (!inPanel) return;
    const onFocus = () => ref.current?.focus();
    const onBlur = () => {
      if (!textRef.current.trim()) onClose();
    };
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    };
  }, [inPanel, onClose]);

  const save = async () => {
    const body = text.trim();
    if (!body || saving) return;
    setSaving(true);
    setError(null);
    const words = body.split(/\s+/).length;
    let entry;
    try {
      entry = await getBridge().createEntry(
        {
          transcript: body,
          durationMs: Math.round((words / WPM) * 60_000),
          fingerprint: [],
          typed: true,
          parentEdge: answering?.entryId ?? null,
        },
        answering?.questionId ?? null,
      );
    } catch (e) {
      // The words stay in the field: a failed save must never cost what was typed.
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : 'could not save');
      setSaving(false);
      return;
    }
    // Saved from here on. Closing first means nothing after this can leave the
    // composer up offering to save the same note twice.
    onClose();

    // Written from outside the app: the canvas takes delivery and the panel
    // goes, same as a recording made from there. Nothing is pulled forward.
    if (inPanel) {
      await handOff({ entry, question: null }).catch(() => false);
      return;
    }

    upsertEntry(entry);
    if (!answering) {
      openEntry(entry.id);
      return;
    }
    // A reply closes the question it answers and stays on the note it answers,
    // exactly as a spoken one does (store/capture.ts stopRecording).
    const state = useApp.getState();
    const questions = new Map(state.questions);
    const prior = questions.get(answering.entryId);
    if (prior) {
      const target =
        answering.questionId ?? prior.find((q) => !q.answered && !q.dismissed)?.id;
      questions.set(
        answering.entryId,
        prior.map((q) => (q.id === target ? { ...q, answered: true } : q)),
      );
    }
    useApp.setState({ questions });
    const edges = await getBridge().listEdges().catch(() => null);
    if (edges) useApp.setState({ edges });
  };

  return (
    <div className={`${styles.composer} ${inPanel ? styles.inPanel : ''}`}>
      <div className={styles.head}>
        <span className={styles.label}>{answering ? 'reply' : 'note'}</span>
        <span className={styles.hint}>
          {answering
            ? 'joins on to the note you have open'
            : 'read back and filed like any other note'}
        </span>
      </div>

      <textarea
        ref={ref}
        className={`${styles.field} selectable`}
        value={text}
        rows={3}
        placeholder={
          answering ? 'what you think about it now' : 'whatever you are thinking about'
        }
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
        {error ? (
          <span className={styles.hint} role="alert">
            not saved — {error}
          </span>
        ) : (
          <span className={styles.hint}>
            <kbd className={styles.kbd}>Enter</kbd> saves ·{' '}
            <kbd className={styles.kbd}>Shift</kbd>+<kbd className={styles.kbd}>Enter</kbd> new line
          </span>
        )}
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
