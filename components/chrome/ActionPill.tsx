'use client';

import { useEffect, useRef, useState } from 'react';
import { exportJson, exportTranscripts, parseImport, type ParsedImport } from '@/lib/corpus-io';
import { useApp } from '@/lib/store';
import styles from './ActionPill.module.css';

const ARM_MS = 4000;

type Menu = 'export' | 'upload' | null;

export default function ActionPill() {
  const entries = useApp((s) => s.entries);
  const edges = useApp((s) => s.edges);
  const questions = useApp((s) => s.questions);
  const clearSample = useApp((s) => s.clearSample);
  const setSampleLoaded = useApp((s) => s.setSampleLoaded);
  const importCorpus = useApp((s) => s.importCorpus);
  const composing = useApp((s) => s.composing);
  const setComposing = useApp((s) => s.setComposing);
  const relayout = useApp((s) => s.relayout);

  const [menu, setMenu] = useState<Menu>(null);
  const [armed, setArmed] = useState(false);
  const [pending, setPending] = useState<ParsedImport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tidying, setTidying] = useState(false);

  const wrapRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!menu && !error) return;
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setMenu(null);
        setError(null);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menu, error]);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  const count = entries.size;
  const all = () => [...entries.values()];

  // Two presses rather than a dialog: the corpus is the whole record, and a
  // modal here would be the first one in the app.
  function onDelete() {
    if (!armed) {
      setArmed(true);
      timer.current = setTimeout(() => setArmed(false), ARM_MS);
      return;
    }
    if (timer.current) clearTimeout(timer.current);
    setArmed(false);
    void clearSample().then(() => setSampleLoaded(false));
  }

  async function onFile(file: File) {
    const parsed = parseImport(await file.text());
    if ('error' in parsed) {
      setPending(null);
      setError(parsed.error);
      setMenu(null);
      return;
    }
    setError(null);
    setPending(parsed);
    setMenu('upload');
  }

  function run(mode: 'merge' | 'replace') {
    if (!pending) return;
    void importCorpus(pending, mode);
    setPending(null);
    setMenu(null);
  }

  return (
    <div className={styles.pill} ref={wrapRef}>
      {menu === 'export' && (
        <div className={styles.menu}>
          <button
            type="button"
            className={styles.menuItem}
            onClick={() => {
              exportTranscripts(all());
              setMenu(null);
            }}
          >
            <span className={styles.menuLabel}>markdown</span>
            <span className={styles.menuHint}>transcripts, as written</span>
          </button>
          <button
            type="button"
            className={styles.menuItem}
            onClick={() => {
              exportJson(all(), edges, [...questions.values()]);
              setMenu(null);
            }}
          >
            <span className={styles.menuLabel}>json</span>
            <span className={styles.menuHint}>everything, re-uploadable</span>
          </button>
        </div>
      )}

      {menu === 'upload' && pending && (
        <div className={styles.menu}>
          <span className={styles.menuHead}>
            {pending.entries.length} entries · {pending.edges.length} edges
          </span>
          <button type="button" className={styles.menuItem} onClick={() => run('merge')}>
            <span className={styles.menuLabel}>merge</span>
            <span className={styles.menuHint}>keep what is here, add the rest</span>
          </button>
          <button type="button" className={styles.menuItem} onClick={() => run('replace')}>
            <span className={styles.menuLabel}>replace</span>
            <span className={styles.menuHint}>this file becomes the corpus</span>
          </button>
        </div>
      )}

      {error && <div className={styles.menu}><span className={styles.menuHead}>{error}</span></div>}

      {/* §4 — the typed path needs a visible entrance, not just a hotkey. */}
      <button
        type="button"
        className={styles.action}
        onClick={() => setComposing(!composing)}
        aria-label="Add a typed note"
        title="Add a typed note"
      >
        +
      </button>

      <span className={styles.divider} aria-hidden />

      {/* Placement is frozen on purpose (§5.1); this is the door out of it when
          the field has drifted into a tangle. Explicit, and undoable by dragging. */}
      <button
        type="button"
        className={styles.action}
        disabled={count < 2 || tidying}
        onClick={() => {
          setTidying(true);
          void relayout().finally(() => setTidying(false));
        }}
        title="Pull linked notes together without overlapping any titles"
      >
        {tidying ? 'tidying' : 'tidy'}
      </button>

      <button
        type="button"
        className={styles.action}
        disabled={count === 0}
        onClick={() => setMenu(menu === 'export' ? null : 'export')}
      >
        export
      </button>

      <button
        type="button"
        className={styles.action}
        onClick={() => {
          setError(null);
          fileRef.current?.click();
        }}
      >
        upload
      </button>

      <input
        ref={fileRef}
        type="file"
        accept="application/json,.json"
        className={styles.file}
        onChange={(e) => {
          const f = e.target.files?.[0];
          e.target.value = '';
          if (f) void onFile(f);
        }}
      />

      <span className={styles.divider} aria-hidden />

      <button
        type="button"
        className={armed ? styles.armed : styles.action}
        disabled={count === 0}
        onClick={onDelete}
      >
        {armed ? 'clear — press again' : 'clear'}
      </button>
    </div>
  );
}
