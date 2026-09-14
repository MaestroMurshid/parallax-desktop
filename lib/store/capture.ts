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
  | 'saved'
  /** The backend refused. Said briefly, then back to idle, so a failure can
   *  never leave the always-on-top panel stuck on "transcribing". */
  | 'failed';

/** Past this, the question is not worth holding capture open for; it surfaces
 *  on the entry later instead (§4). Holding is only right for short ones. */
export const RELEASE_AFTER_MS = 150_000;
/** How long "recorded" stays up. Long enough to read out of the corner of your
 *  eye, short enough that it never becomes something to dismiss. */
export const SAVED_MS = 1_400;
/** §4 — escape means "discard" while recording and "leave it" when stopped,
 *  which is a muscle-memory trap. No confirmation dialog; an undo window instead. */
export const UNDO_WINDOW_MS = 60_000;
/** Long enough to read a one-line reason, then the panel gets out of the way. */
export const FAILED_MS = 4_000;

export interface CaptureSlice {
  captureState: CaptureState;
  startedAt: number | null;
  elapsedMs: number;
  /** Set while recording an answer: the entry this becomes a layer on (§6.2). */
  answeringEntryId: string | null;
  /** Which question the recording is answering, when the user picked one. */
  answeringQuestionId: string | null;
  discardedAt: number | null;
  /** Why the last capture failed, shown while `captureState` is 'failed'. */
  captureError: string | null;
  /**
   * Set when escape is pressed with transcription already in flight.
   *
   * Transcription cannot be called off once it has started -- there is no
   * cancel token through whisper, and the samples are already handed over -- so
   * what is cancelled is the note, not the work. The entry is deleted the
   * moment it arrives, which is what "cancel" means to someone who has decided
   * they do not want it.
   */
  cancelRequested: boolean;

  startRecording(answeringEntryId?: string | null, answeringQuestionId?: string | null): Promise<void>;
  stopRecording(): Promise<void>;
  discardRecording(): Promise<void>;
  /** Escape, whichever half of capture is running. */
  cancelCapture(): Promise<void>;
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
  function fail(what: string, error: unknown): void {
    const reason = typeof error === 'string' ? error : error instanceof Error ? error.message : '';
    set({
      captureState: 'failed',
      captureError: reason ? `${what}: ${reason}` : what,
      startedAt: null,
      elapsedMs: 0,
      answeringEntryId: null,
      answeringQuestionId: null,
      cancelRequested: false,
    });
    setTimeout(() => {
      if (get().captureState === 'failed') set({ captureState: 'idle', captureError: null });
    }, FAILED_MS);
  }

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
  answeringQuestionId: null,
  discardedAt: null,
  captureError: null,
  cancelRequested: false,

  async startRecording(answeringEntryId = null, answeringQuestionId = null) {
    const now = get().captureState;
    if (now !== 'idle' && now !== 'failed') return;
    try {
      await getBridge().startRecording();
    } catch (e) {
      fail("couldn't start recording", e);
      return;
    }
    set({
      captureError: null,
      cancelRequested: false,
      captureState: 'recording',
      startedAt: Date.now(),
      elapsedMs: 0,
      answeringEntryId,
      answeringQuestionId,
    });
  },

  async stopRecording() {
    if (get().captureState !== 'recording') return;
    const long = get().elapsedMs >= RELEASE_AFTER_MS;
    set({ captureState: 'transcribing' });

    const answering = get().answeringEntryId;
    const targeted = get().answeringQuestionId;
    let entry: Entry;
    try {
      entry = await getBridge().stopRecording(answering, targeted);
    } catch (e) {
      fail("couldn't save the recording", e);
      return;
    }

    // Escape landed while this was in flight. Delete rather than keep-and-hide:
    // the audio goes with the row, and a note the user cancelled is not
    // something to leave lying in the corpus for them to find later.
    if (get().cancelRequested) {
      // A failed delete keeps the note rather than keeping the panel up.
      await getBridge().deleteEntry(entry.id).catch(() => undefined);
      set({
        captureState: 'idle',
        startedAt: null,
        elapsedMs: 0,
        answeringEntryId: null,
        answeringQuestionId: null,
        cancelRequested: false,
      });
      return;
    }

    get().upsertEntry(entry);

    // An answer closes the question it replies to, then goes on to be a note
    // like any other: drawn on the canvas, joined to what it answers, and
    // asked its own question. Speaking is how the thread continues, which is
    // the point at which one entry becomes two rather than a conversation.
    if (answering) {
      // The one they chose, or the oldest still open if they just hit the key.
      const questions = new Map(get().questions);
      const prior = questions.get(answering);
      if (prior) {
        const target = targeted ?? prior.find((q) => !q.answered && !q.dismissed)?.id;
        questions.set(
          answering,
          prior.map((q) => (q.id === target ? { ...q, answered: true } : q)),
        );
      }
      // The answer is already saved; a question or edge read that fails only
      // costs what is shown now, and the next refresh brings it back.
      const own = long ? null : await getBridge().getQuestion(entry.id).catch(() => null);
      if (own) questions.set(entry.id, [own]);
      set({
        questions,
        edges: await getBridge().listEdges().catch(() => get().edges),
        captureState: 'idle',
        answeringEntryId: null,
        answeringQuestionId: null,
      });
      return;
    }

    // A long recording skips the question rather than making you wait on one
    // with the recorder still up; the background pass surfaces it later (§4).
    const question = long ? null : await getBridge().getQuestion(entry.id).catch(() => null);
    if (question) {
      const questions = new Map(get().questions);
      questions.set(entry.id, [question]);
      set({ questions });
    }

    set({ startedAt: null, elapsedMs: 0 });
    try {
      await land(entry, question);
    } catch {
      // Only the hand-off between windows failed; the note is saved.
      set({ captureState: 'idle' });
    }
  },

  async discardRecording() {
    if (get().captureState !== 'recording') return;
    try {
      await getBridge().discardRecording();
    } catch (e) {
      fail("couldn't discard", e);
      return;
    }
    set({
      captureState: 'idle',
      startedAt: null,
      elapsedMs: 0,
      answeringEntryId: null,
      discardedAt: Date.now(),
    });
  },

  /**
   * One key for both halves, because the person pressing it is answering the
   * same question either way -- they have decided they do not want this note,
   * and which stage the machinery happens to be in is not their problem.
   */
  async cancelCapture() {
    const state = get().captureState;
    if (state === 'recording') {
      await get().discardRecording();
      return;
    }
    // Nothing to undo here: discard keeps its sixty-second window because the
    // recording still exists to put back, and a deleted entry does not.
    if (state === 'transcribing') set({ cancelRequested: true });
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
