'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge, initBridge } from '@/lib/bridge';
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
  const setPlaybackMs = useApp((s) => s.setPlaybackMs);
  const stop = useApp((s) => s.stopPlayback);
  const openEntry = useApp((s) => s.openEntry);

  const audio = useRef<HTMLAudioElement | null>(null);
  // Null until the bytes are known to exist or not; the simulated clock only
  // runs once we know there is nothing to play.
  const [real, setReal] = useState<boolean | null>(null);

  useEffect(() => {
    if (!id) return setReal(null);
    let url: string | null = null;
    let cancelled = false;

    void (async () => {
      await initBridge();
      const bytes = await getBridge().readAudio(id);
      if (cancelled) return;
      if (!bytes || !audio.current) return setReal(false);

      url = URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' }));
      audio.current.src = url;
      setReal(true);
      try {
        await audio.current.play();
      } catch {
        // Autoplay refused or the file will not decode: fall back to the clock
        // rather than leaving a pill that never moves.
        setReal(false);
      }
    })();

    return () => {
      cancelled = true;
      audio.current?.pause();
      if (url) URL.revokeObjectURL(url);
    };
  }, [id]);

  // The simulated clock, for a typed entry or a recording that is gone.
  useEffect(() => {
    if (!id || real !== false) return;
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [id, real, tick]);

  if (!entry) return null;
  const pct = Math.min(100, (playbackMs / Math.max(entry.durationMs, 1)) * 100);

  return (
    <div className={styles.pill} role="status">
      <audio
        ref={audio}
        onTimeUpdate={(e) => setPlaybackMs(Math.round(e.currentTarget.currentTime * 1000))}
        onEnded={stop}
        hidden
      />
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
