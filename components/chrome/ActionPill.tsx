'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge, type UploadPreview } from '@/lib/bridge';
import { transcriptsMarkdown } from '@/lib/corpus-io';
import { useApp } from '@/lib/store';
import styles from './ActionPill.module.css';

const ARM_MS = 4000;

type Menu = 'export' | 'upload' | null;

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** A path is long and the part that matters is the end of it. */
function shortPath(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts.length > 3 ? `…/${parts.slice(-2).join('/')}` : path;
}

export default function ActionPill() {
  const entries = useApp((s) => s.entries);
  const clearAll = useApp((s) => s.clearAll);
  const setSampleLoaded = useApp((s) => s.setSampleLoaded);
  const applyUpload = useApp((s) => s.applyUpload);
  const composing = useApp((s) => s.composing);
  const setComposing = useApp((s) => s.setComposing);
  const relayout = useApp((s) => s.relayout);
  const fitAll = useApp((s) => s.fitAll);

  const [menu, setMenu] = useState<Menu>(null);
  const [armed, setArmed] = useState(false);
  const [pending, setPending] = useState<UploadPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** What the last export or upload did, said once, in words. */
  const [notice, setNotice] = useState<string | null>(null);
  /** A dialog is open or a file is being written; the buttons wait. */
  const [busy, setBusy] = useState(false);
  const [tidying, setTidying] = useState(false);

  const wrapRef = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!menu && !error && !notice) return;
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setMenu(null);
        setError(null);
        setNotice(null);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menu, error, notice]);

  useEffect(() => () => {
    if (noticeTimer.current) clearTimeout(noticeTimer.current);
  }, []);

  function say(text: string) {
    setError(null);
    setNotice(text);
    if (noticeTimer.current) clearTimeout(noticeTimer.current);
    noticeTimer.current = setTimeout(() => setNotice(null), 8000);
  }

  /** Runs one dialog-backed action, and says what went wrong in its own words
   *  rather than leaving a button that did nothing. */
  async function act(work: () => Promise<void>) {
    setMenu(null);
    setNotice(null);
    setError(null);
    setBusy(true);
    try {
      await work();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  function exportArchive(withAudio: boolean) {
    void act(async () => {
      const done = await getBridge().exportArchive(withAudio);
      if (!done) return;
      const recordings = withAudio ? ` and ${plural(done.audio, 'recording')}` : '';
      const missing = done.missingAudio > 0 ? ` — ${plural(done.missingAudio, 'recording')} not found` : '';
      say(`${plural(done.notes, 'note')}${recordings} saved to ${shortPath(done.path)}${missing}`);
    });
  }

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  const count = entries.size;
  const all = () => [...entries.values()];

  // Two presses rather than a dialog: the corpus is the whole record, and a
  // modal here would be the first one in the app. It is also the whole record
  // that goes — clearing only the sample here left everything you had recorded
  // behind, under a button that just says `clear`.
  function onDelete() {
    if (!armed) {
      setArmed(true);
      timer.current = setTimeout(() => setArmed(false), ARM_MS);
      return;
    }
    if (timer.current) clearTimeout(timer.current);
    setArmed(false);
    void clearAll().then(() => setSampleLoaded(false));
  }

  function pickUpload() {
    void act(async () => {
      setPending(null);
      const preview = await getBridge().pickUpload();
      if (!preview) return;
      setPending(preview);
      setMenu('upload');
    });
  }

  function run(mode: 'merge' | 'replace') {
    const preview = pending;
    if (!preview) return;
    setPending(null);
    void act(async () => {
      await applyUpload(mode);
      say(`${plural(preview.notes, 'note')} uploaded from ${preview.fileName}`);
    });
  }

  return (
    <div className={styles.pill} ref={wrapRef}>
      {menu === 'export' && (
        <div className={styles.menu}>
          <button type="button" className={styles.menuItem} onClick={() => exportArchive(false)}>
            <span className={styles.menuLabel}>notes</span>
            <span className={styles.menuHint}>every note as a file, in one zip</span>
          </button>
          <button type="button" className={styles.menuItem} onClick={() => exportArchive(true)}>
            <span className={styles.menuLabel}>notes + audio</span>
            <span className={styles.menuHint}>with the recordings, re-uploadable</span>
          </button>
          <button
            type="button"
            className={styles.menuItem}
            onClick={() =>
              void act(async () => {
                const path = await getBridge().exportTranscripts(transcriptsMarkdown(all()));
                if (path) say(`transcripts saved to ${shortPath(path)}`);
              })
            }
          >
            <span className={styles.menuLabel}>markdown</span>
            <span className={styles.menuHint}>transcripts, as written</span>
          </button>
        </div>
      )}

      {menu === 'upload' && pending && (
        <div className={styles.menu}>
          <span className={styles.menuHead}>
            {plural(pending.notes, 'note')} · {plural(pending.edges, 'edge')}
            {pending.recordings > 0 && ` · ${plural(pending.recordings, 'recording')}`}
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
      {notice && !error && !menu && (
        <div className={styles.menu} role="status">
          <span className={styles.menuHead}>{notice}</span>
        </div>
      )}

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

      {/* The camera's only escape hatch (§5.1): a note off-screen can be
          brought back by moving the view, never by re-solving the field. */}
      <button
        type="button"
        className={styles.action}
        disabled={count === 0}
        onClick={() => fitAll()}
        title="Bring every note on screen"
      >
        fit
      </button>

      <button
        type="button"
        className={styles.action}
        disabled={count === 0 || busy}
        onClick={() => {
          setNotice(null);
          setError(null);
          setMenu(menu === 'export' ? null : 'export');
        }}
      >
        export
      </button>

      <button type="button" className={styles.action} disabled={busy} onClick={pickUpload}>
        upload
      </button>

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
