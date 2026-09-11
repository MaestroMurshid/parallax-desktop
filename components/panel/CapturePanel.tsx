'use client';

import { useEffect, useState } from 'react';
import { getBridge, initBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import Equalizer from './Equalizer';
import styles from './CapturePanel.module.css';

function elapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

/**
 * §4 said bars and elapsed time and nothing else, on the grounds that reading
 * your own words back makes you self-edit next time. Reversed deliberately: with
 * no transcript and bars that were not moving, a working microphone was
 * indistinguishable from a muted one, and not knowing whether it heard you is
 * worse than seeing a rough draft of what it heard.
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
  const [partial, setPartial] = useState('');

  useEffect(() => {
    if (state !== 'recording') return;
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [state, tick]);

  // A pass costs about a thirty-fifth of what has been said, so this asks again
  // only once the last answer is in rather than on a fixed clock.
  useEffect(() => {
    if (state !== 'recording') return setPartial('');
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;

    const poll = async () => {
      try {
        await initBridge();
        const text = await getBridge().partialTranscript();
        if (!stopped && text) setPartial(text);
      } catch {
        // No model yet, or nothing recording. The bars still say it is live.
      }
      if (!stopped) timer = setTimeout(poll, 1200);
    };
    void poll();

    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [state]);

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
      {state === 'recording' && partial && (
        <p className={styles.partial} aria-live="polite">
          {partial}
        </p>
      )}
    </div>
  );
}
