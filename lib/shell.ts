/**
 * The desktop-shell seam: where the webviews are, as opposed to what is in
 * them (that is lib/bridge). §4 puts capture in its own borderless always-on-top
 * window, so "recording finished" has to cross a window boundary that does not
 * exist in a browser tab. Everything here no-ops outside Tauri, which is what
 * keeps `next dev` running the same mockup.
 */

import { isTauri } from '@/lib/bridge';
import type { Entry, Question } from '@/lib/types';

/** Rust fires this on the global shortcut, before the panel is shown (§4). */
export const HOTKEY_EVENT = 'capture://hotkey';
/** The panel's hand-off to the main window once transcription lands. */
export const HANDOFF_EVENT = 'capture://handoff';

export type Unsubscribe = () => void;

/**
 * What crosses the window boundary. The question rides along because the panel
 * is the one that asked for it — the main window's bridge never saw the request,
 * and under the fixture backend each window has its own corpus in memory.
 */
export interface HandOff {
  entry: Entry;
  question: Question | null;
}

/** True when this webview is the floating capture panel, not the main window. */
export function isPanelWindow(): boolean {
  if (typeof window === 'undefined') return false;
  return new URLSearchParams(window.location.search).get('window') === 'panel';
}

/**
 * Tauri event listeners register asynchronously, so the unsubscribe has to
 * survive being called before registration completes — same shape as the
 * bridge's subscribe, and for the same reason.
 */
function subscribe<T>(event: string, cb: (payload: T) => void): Unsubscribe {
  if (!isTauri()) return () => {};

  let stop: (() => void) | null = null;
  let cancelled = false;

  void import('@tauri-apps/api/event')
    .then(({ listen }) => listen<T>(event, (e) => cb(e.payload)))
    .then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });

  return () => {
    cancelled = true;
    stop?.();
  };
}

/** The global shortcut fired. In Tauri this is the only thing that starts a
 *  recording — the in-page key handler stands down so one press is one take. */
export function onHotkey(cb: () => void): Unsubscribe {
  return subscribe<null>(HOTKEY_EVENT, () => cb());
}

/** Main window: an entry finished recording in the panel. */
export function onHandOff(cb: (h: HandOff) => void): Unsubscribe {
  return subscribe<HandOff>(HANDOFF_EVENT, cb);
}

/**
 * Bring the panel up. Paired with hidePanel so the window is on screen exactly
 * while capture is running and never a moment either side of it.
 */
export async function showPanel(): Promise<void> {
  if (!isTauri() || !isPanelWindow()) return;
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('show_capture');
}

/**
 * Put the panel away. Called whenever capture returns to idle, so the window's
 * visibility follows the state machine rather than depending on every exit path
 * remembering to hide it — discard has no hand-off to ride out on.
 */
export async function hidePanel(): Promise<void> {
  if (!isTauri() || !isPanelWindow()) return;
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('hide_capture');
}

/**
 * Recording is over. The panel tells the canvas what it caught and leaves it
 * there — it does not pull the window forward. You recorded that while reading
 * something else; being yanked out of it is exactly what the global hotkey is
 * for avoiding. The entry is waiting on the canvas whenever you next look.
 *
 * Returns false in a browser tab and in the canvas window, where there is no
 * boundary to cross and the caller should open the entry itself.
 */
export async function handOff(payload: HandOff): Promise<boolean> {
  if (!isTauri() || !isPanelWindow()) return false;
  const { emit } = await import('@tauri-apps/api/event');
  await emit(HANDOFF_EVENT, payload);
  return true;
}
