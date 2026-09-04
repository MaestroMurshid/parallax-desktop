import type { StateCreator } from 'zustand';
import type { Entry, Question } from '@/lib/types';
import { getBridge } from '@/lib/bridge';
import { handOff } from '@/lib/shell';
import type { AppState, Mutators } from './index';

/**
 * Capture state machine (§4): hotkey starts recording immediately, panel
 * second — no pause to look at UI before speaking. Amplitude skips the store
 * too; onAmplitude writes DOM refs directly, not 60 re-renders/sec.
 *
 * Capture ends when the transcript exists. It does not linger to show you the
 * result: the panel is a recorder, and the place to read an entry is the entry.
 */
export type CaptureState =
  | 'idle'
  | 'recording'
  | 'transcribing'
  /** Caught from outside the app: says so, briefly, and goes (§4). */
  | 'saved';

/** Past this, the question is not worth holding capture open for; it surfaces
 *  on the entry later instead (§4). Holding is only right for short ones. */
export const RELEASE_AFTER_MS = 150_000;
/** How long "recorded" stays up. Long enough to read out of the corner of your
 *  eye, short enough that it never becomes something to dismiss. */
export const SAVED_MS = 1_400;
/** §4 — escape means "discard" while recording and "leave it" when stopped,
 *  which is a muscle-memory trap. No confirmation dialog; an undo window instead. */
export const UNDO_WINDOW_MS = 60_000;

export interface CaptureSlice {
  captureState: CaptureState;
  startedAt: number | null;
  elapsedMs: number;
  /** Set while recording an answer: the entry this becomes a layer on (§6.2). */
  answeringEntryId: string | null;
  discardedAt: number | null;

  startRecording(answeringEntryId?: string | null): Promise<void>;
  stopRecording(): Promise<void>;
  discardRecording(): Promise<void>;
  undoDiscard(): Promise<void>;
  tickElapsed(): void;
  dismissPanel(): void;
}

export const createCaptureSlice: StateCreator<AppState, Mutators, [], CaptureSlice> = (set, get) => {
  /**
   * Where a finished recording goes, and it depends on where it started.
   *
   * Recorded in the canvas: the transcript is the thing you came for, so it
   * opens. Recorded from the panel — mid-paper, mid-anything — the canvas takes
   * delivery silently and the panel says "recorded" and leaves. Pulling the app
   * forward there would undo the reason for having a global hotkey at all.
   */
  async function land(entry: Entry, question: Question | null): Promise<void> {
    const handed = await handOff({ entry, question });
    if (!handed) {
      set({ captureState: 'idle' });
      get().openEntry(entry.id);
      return;
    }
    set({ captureState: 'saved' });
    setTimeout(() => {
      // Only if nothing else has happened since — a second recording started
      // inside the window owns the state now.
      if (get().captureState === 'saved') set({ captureState: 'idle' });
    }, SAVED_MS);
  }

  return {
  captureState: 'idle',
  startedAt: null,
  elapsedMs: 0,
  answeringEntryId: null,
  discardedAt: null,

  async startRecording(answeringEntryId = null) {
    if (get().captureState !== 'idle') return;
    await getBridge().startRecording();
    set({
      captureState: 'recording',
      startedAt: Date.now(),
      elapsedMs: 0,
      answeringEntryId,
    });
  },

  async stopRecording() {
    if (get().captureState !== 'recording') return;
    const long = get().elapsedMs >= RELEASE_AFTER_MS;
    set({ captureState: 'transcribing' });

    const answering = get().answeringEntryId;
    const entry: Entry = await getBridge().stopRecording(answering);
    get().upsertEntry(entry);

    // An answer thickens its parent's rings and closes the open question; it
    // never gets probed itself, or one question becomes an interrogation (§3.4).
    if (answering) {
      // The oldest unanswered one is the one they just spoke to.
      const questions = new Map(get().questions);
      const prior = questions.get(answering);
      if (prior) {
        let done = false;
        questions.set(answering, prior.map((q) => {
          if (done || q.answered) return q;
          done = true;
          return { ...q, answered: true };
        }));
      }
      // Answering always starts from an entry already open in this window, so
      // there is no window boundary to cross and nothing new to open.
      set({ questions, captureState: 'idle', answeringEntryId: null });
      return;
    }

    // A long recording skips the question rather than making you wait on one
    // with the recorder still up; the background pass surfaces it later (§4).
    const question = long ? null : await getBridge().getQuestion(entry.id);
    if (question) {
      const questions = new Map(get().questions);
      questions.set(entry.id, [question]);
      set({ questions });
    }

    set({ startedAt: null, elapsedMs: 0 });
    await land(entry, question);
  },

  async discardRecording() {
    if (get().captureState !== 'recording') return;
    await getBridge().discardRecording();
    set({
      captureState: 'idle',
      startedAt: null,
      elapsedMs: 0,
      answeringEntryId: null,
      discardedAt: Date.now(),
    });
  },

  async undoDiscard() {
    const at = get().discardedAt;
    if (!at || Date.now() - at > UNDO_WINDOW_MS) return;
    const entry = await getBridge().undoDiscard();
    if (!entry) return;
    get().upsertEntry(entry);
    set({ discardedAt: null });
    await land(entry, null);
  },

  tickElapsed() {
    const startedAt = get().startedAt;
    if (startedAt === null || get().captureState !== 'recording') return;
    set({ elapsedMs: Date.now() - startedAt });
  },

  dismissPanel() {
    // Ignoring is free. Escape leaves it. Not a decision, not a dismissal (§3.4).
    set({ captureState: 'idle', startedAt: null, elapsedMs: 0, answeringEntryId: null });
  },
  };
};
