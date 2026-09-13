'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge, initBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import styles from './PlayerPill.module.css';

function clock(ms: number): string {
  const t = Math.floor(ms / 1000);
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
}

/** What one press of rewind or forward is worth. Long enough to clear a
 *  sentence you already heard, short enough that two presses are not a
 *  commitment -- a spoken note is a minute, not an album. */
const SKIP_MS = 10_000;

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
  const [paused, setPaused] = useState(false);
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

  // The simulated clock, for a typed entry or a recording that is gone. It
  // stops while paused for the same reason the audio does: the transport says
  // what is happening, so it has to be telling the truth for both kinds.
  useEffect(() => {
    if (!id || real !== false || paused) return;
    const timer = setInterval(tick, 200);
    return () => clearInterval(timer);
  }, [id, real, paused, tick]);

  // A new note starts playing, so the transport resets with it.
  useEffect(() => setPaused(false), [id]);

  if (!entry) return null;
  const pct = Math.min(100, (playbackMs / Math.max(entry.durationMs, 1)) * 100);

  /**
   * Seeking, over whichever clock is running.
   *
   * The audio element is authoritative when there is a file: writing
   * `currentTime` makes it emit `timeupdate`, which is what moves the store, so
   * setting both here would fight it. Without a file there is no element to
   * ask, and the store is the only clock there is.
   */
  const seekTo = (ms: number) => {
    const clamped = Math.max(0, Math.min(entry.durationMs, ms));
    if (real && audio.current) {
      audio.current.currentTime = clamped / 1000;
      return;
    }
    setPlaybackMs(clamped);
  };

  const togglePlay = () => {
    const next = !paused;
    setPaused(next);
    if (!real || !audio.current) return;
    if (next) audio.current.pause();
    else void audio.current.play().catch(() => setPaused(true));
  };

  return (
    <div className={styles.pill} role="status">
      <audio
        ref={audio}
        onTimeUpdate={(e) => setPlaybackMs(Math.round(e.currentTarget.currentTime * 1000))}
        onEnded={stop}
        hidden
      />
      {/* Seekable, and wide enough to hit. It was a two-pixel line at the very
          bottom edge, which reads as progress but cannot be used as a control. */}
      <button
        type="button"
        className={styles.track}
        aria-label="Seek"
        onClick={(e) => {
          const box = e.currentTarget.getBoundingClientRect();
          seekTo(((e.clientX - box.left) / box.width) * entry.durationMs);
        }}
      >
        <span className={styles.fill} style={{ width: `${pct}%` }} />
        <span className={styles.head} style={{ left: `${pct}%` }} aria-hidden />
      </button>

      <div className={styles.transport}>
        <button
          type="button"
          className={styles.control}
          onClick={() => seekTo(playbackMs - SKIP_MS)}
          aria-label="Back ten seconds"
          title="back 10s"
        >
          <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden>
            <path d="M11 6 3 12l8 6V6z" fill="currentColor" />
            <path d="M21 6l-8 6 8 6V6z" fill="currentColor" />
          </svg>
        </button>

        <button
          type="button"
          className={styles.play}
          onClick={togglePlay}
          aria-label={paused ? 'Play' : 'Pause'}
          title={paused ? 'play' : 'pause'}
        >
          {paused ? (
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden>
              <path d="M7 4.5 19.5 12 7 19.5v-15z" fill="currentColor" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden>
              <rect x="6.5" y="4.5" width="4" height="15" fill="currentColor" />
              <rect x="13.5" y="4.5" width="4" height="15" fill="currentColor" />
            </svg>
          )}
        </button>

        <button
          type="button"
          className={styles.control}
          onClick={() => seekTo(playbackMs + SKIP_MS)}
          aria-label="Forward ten seconds"
          title="forward 10s"
        >
          <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden>
            <path d="M13 6l8 6-8 6V6z" fill="currentColor" />
            <path d="M3 6l8 6-8 6V6z" fill="currentColor" />
          </svg>
        </button>
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
