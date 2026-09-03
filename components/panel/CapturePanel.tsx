'use client';

import { useEffect } from 'react';
import { useApp } from '@/lib/store';
import Equalizer from './Equalizer';
import styles from './CapturePanel.module.css';

function elapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/**
 * §4 — recording shows bars and elapsed time and nothing else. No transcript on
 * screen: reading your own words back makes you self-edit next time.
 *
 * It leaves as soon as the transcript exists. Holding the recorder up to show
 * you the result made the panel a reading surface it was never shaped to be,
 * and left the question stranded in a window you had already walked away from —
 * the entry view says the same things with room to say them.
 */
export default function CapturePanel() {
  const state = useApp((s) => s.captureState);
  const elapsedMs = useApp((s) => s.elapsedMs);
  const tick = useApp((s) => s.tickElapsed);

  useEffect(() => {
    if (state !== 'recording') return;
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [state, tick]);

  if (state === 'idle') return null;

  return (
    <div className={styles.panel} role="status">
      {/* Amplitude stops at zero when recording ends, so the bars settle on
          their own — motion still means exactly one thing: audio now (§8). */}
      <Equalizer />
      <div className={styles.elapsed}>
        {state === 'recording' && elapsed(elapsedMs)}
        {state === 'transcribing' && 'transcribing'}
        {/* Confirmation, not a result. It says the words are safe and then it
            goes; reading them back is what the entry is for. */}
        {state === 'saved' && 'recorded'}
      </div>
    </div>
  );
}
