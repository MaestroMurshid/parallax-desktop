'use client';

import { useEffect } from 'react';
import { useApp } from '@/lib/store';
import styles from './PlayerPill.module.css';

function clock(ms: number): string {
  const t = Math.floor(ms / 1000);
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
}

/**
 * Playback is the one moving thing in the app (§8), so it gets its own pill
 * rather than hiding inside the entry sheet — you can close the sheet and it
 * keeps playing.
 */
export default function PlayerPill() {
  const id = useApp((s) => s.playingEntryId);
  const entry = useApp((s) => (id ? s.entries.get(id) : undefined));
  const playbackMs = useApp((s) => s.playbackMs);
  const tick = useApp((s) => s.tickPlayback);
  const stop = useApp((s) => s.stopPlayback);
  const openEntry = useApp((s) => s.openEntry);

  useEffect(() => {
    if (!id) return;
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [id, tick]);

  if (!entry) return null;
  const pct = Math.min(100, (playbackMs / Math.max(entry.durationMs, 1)) * 100);

  return (
    <div className={styles.pill} role="status">
      <div className={styles.track} aria-hidden>
        <div className={styles.fill} style={{ width: `${pct}%` }} />
      </div>
      <button type="button" className={styles.title} onClick={() => openEntry(entry.id)}>
        {entry.title}
      </button>
      <span className={styles.time}>
        {clock(playbackMs)} / {clock(entry.durationMs)}
      </span>
      <button type="button" className={styles.stop} onClick={stop} aria-label="Stop playback">
        stop
      </button>
    </div>
  );
}
