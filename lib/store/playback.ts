import type { StateCreator } from 'zustand';
import type { AppState, Mutators } from './index';

/**
 * §8 — audio is the one thing in this app that moves, so playback state is
 * shared: the pill shows it and the canvas can mark which entry is speaking.
 * A typed note has no audio, so its clock is simulated against durationMs.
 */
export interface PlaybackSlice {
  playingEntryId: string | null;
  playbackMs: number;
  playEntry(id: string): void;
  stopPlayback(): void;
  /** Advances the simulated clock. Only used when there is no real audio. */
  tickPlayback(): void;
  /** Driven by the audio element, which is the clock when bytes exist. */
  setPlaybackMs(ms: number): void;
}

export const createPlaybackSlice: StateCreator<AppState, Mutators, [], PlaybackSlice> = (
  set,
  get,
) => ({
  playingEntryId: null,
  playbackMs: 0,

  playEntry(id) {
    const entry = get().entries.get(id);
    // Typed entries have no audio to play, so the pill never appears for them.
    if (!entry || entry.audioPath === null) return;
    set({ playingEntryId: id, playbackMs: 0 });
  },

  stopPlayback() {
    set({ playingEntryId: null, playbackMs: 0 });
  },

  setPlaybackMs(ms) {
    if (!get().playingEntryId) return;
    set({ playbackMs: ms });
  },

  tickPlayback() {
    const id = get().playingEntryId;
    if (!id) return;
    const entry = get().entries.get(id);
    if (!entry) return set({ playingEntryId: null, playbackMs: 0 });
    const next = get().playbackMs + 200;
    if (next >= entry.durationMs) return set({ playingEntryId: null, playbackMs: 0 });
    set({ playbackMs: next });
  },
});
