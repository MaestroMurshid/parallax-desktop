'use client';

import { useApp } from '@/lib/store';
import type { CaptureMode } from '@/lib/types';
import styles from './EmptyState.module.css';

/** The capture panel's own equalizer, at rest. Static, because motion in this
 *  app means exactly one thing: audio is running now (§8). */
const RESTING = [18, 34, 52, 78, 96, 70, 44, 60, 26];

/**
 * The empty canvas shows the instrument, not a message — same card the capture
 * panel uses, waiting. §12's expectation-setting sits under it, quieter.
 */
export default function EmptyState({ hotkey, mode }: { hotkey: string; mode: CaptureMode }) {
  const loadSample = useApp((s) => s.loadSample);
  const setSampleLoaded = useApp((s) => s.setSampleLoaded);
  const capture = useApp((s) => s.captureState);
  const composing = useApp((s) => s.composing);

  // The live panel wears this same card, so the dormant one steps aside rather
  // than sitting behind it looking like a duplicate. Same for the composer.
  if (capture !== 'idle' || composing) return null;

  return (
    <div className={styles.empty}>
      <div className={styles.card}>
        {/* The instrument the key opens: an empty field for text, the resting
            bars for voice. Neither moves -- motion still means audio (§8). */}
        {mode === 'text' ? (
          <div className={styles.field} aria-hidden />
        ) : (
          <div className={styles.equalizer} aria-hidden>
            {RESTING.map((h, i) => (
              <span key={i} className={styles.bar} style={{ height: `${h}%` }} />
            ))}
          </div>
        )}
        <p className={styles.prompt}>
          Press <kbd className={styles.kbd}>{hotkey}</kbd> and {mode === 'text' ? 'write' : 'talk'}.
        </p>
        <p className={styles.sub}>Anywhere, even with this window behind something else.</p>
      </div>

      <p className={styles.body}>
        This works on things you have a position on and might change your mind about.
        {mode === 'text' ? 'Write' : 'Record'} a few, then come back in a month — the point is what your March self
        says to your November one.
      </p>

      <button
        type="button"
        className={styles.sample}
        onClick={async () => {
          await loadSample();
          setSampleLoaded(true);
        }}
      >
        or look around a sample corpus
      </button>
    </div>
  );
}
