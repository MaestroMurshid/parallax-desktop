'use client';

import { useEffect, useState } from 'react';
import Canvas from '@/components/canvas/Canvas';
import EmptyState from '@/components/canvas/EmptyState';
import ConnectPicker from '@/components/canvas/ConnectPicker';
import RelationPicker from '@/components/canvas/RelationPicker';
import SparseNotice from '@/components/canvas/SparseNotice';
import ActionPill from '@/components/chrome/ActionPill';
import PlayerPill from '@/components/chrome/PlayerPill';
import StatusBar from '@/components/chrome/StatusBar';
import TopBar from '@/components/chrome/TopBar';
import EntryView from '@/components/entry/EntryView';
import FileView from '@/components/entry/FileView';
import ChatPanel from '@/components/chat/ChatPanel';
import ListView, { type RoleFilter } from '@/components/list/ListView';
import Sidebar from '@/components/list/Sidebar';
import Onboarding from '@/components/onboarding/Onboarding';
import CapturePanel from '@/components/panel/CapturePanel';
import TypedComposer from '@/components/panel/TypedComposer';
import SettingsPanel from '@/components/settings/SettingsPanel';
import TaskList from '@/components/tasks/TaskList';
import { getBridge, initBridge, isTauri } from '@/lib/bridge';
import { broadcastSettings, hidePanel, isPanelWindow, onDiscardHotkey, onHandOff, onHotkey, onSettingsChange, showPanel } from '@/lib/shell';
import { useApp } from '@/lib/store';
import type { Settings } from '@/lib/types';
import styles from './page.module.css';

function matchesHotkey(e: KeyboardEvent, hotkey: string): boolean {
  const parts = hotkey.toLowerCase().split('+');
  const key = parts[parts.length - 1] ?? '';
  if (parts.includes('ctrl') !== e.ctrlKey) return false;
  if (parts.includes('shift') !== e.shiftKey) return false;
  if (parts.includes('alt') !== e.altKey) return false;
  return key === 'space' ? e.code === 'Space' : e.key.toLowerCase() === key;
}

export default function Page() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [onboarded, setOnboarded] = useState(false);
  /** Decided once at launch. Asked again mid-session, a download finishing or
   *  a model being removed would throw someone out of the app into onboarding. */
  const [needsOnboarding, setNeedsOnboarding] = useState<boolean | null>(null);
  // The capture panel is its own borderless always-on-top Tauri window (§4),
  // pointed at this same page. Same markup, no second route (§9.1). Null until
  // mount, because a static export has no window to ask at prerender time and
  // guessing wrong paints the canvas inside the panel for a frame.
  const [isPanel, setIsPanel] = useState<boolean | null>(null);
  // Decided after mount for the same reason as isPanel: the static export has
  // no window, and the answer must not differ between prerender and Tauri.
  const [outsideShell, setOutsideShell] = useState(false);
  const loaded = useApp((s) => s.loaded);
  const hasEntries = useApp((s) => s.order.length > 0);
  const overlay = useApp((s) => s.overlay);
  const composing = useApp((s) => s.composing);
  const captureState = useApp((s) => s.captureState);
  const view = useApp((s) => s.view);
  // Local to the page: a filter is a way of looking, not a fact about the
  // corpus, and it should not survive a reload the way stored state would.
  const [roleFilter, setRoleFilter] = useState<RoleFilter>('all');
  const setComposing = useApp((s) => s.setComposing);

  useEffect(() => {
    const panel = isPanelWindow();
    setIsPanel(panel);
    if (!isTauri()) {
      setOutsideShell(true);
      return;
    }
    // Lets the stylesheet drop the page surface for this window; the panel is
    // meant to float over other apps, not to be a grey box on the desktop.
    if (panel) document.documentElement.dataset.window = 'panel';
    void (async () => {
      await initBridge();
      const [loadedSettings, setUp] = await Promise.all([
        getBridge().getSettings(),
        getBridge().setupComplete(),
      ]);
      // A development override only; unset, first run is decided by the disk.
      setNeedsOnboarding(process.env.NEXT_PUBLIC_ALWAYS_ONBOARD === '1' || !setUp);
      setSettings(loadedSettings);
      // The canvas and the list draw the register treatment without being
      // handed the whole Settings object, so the store carries this one field.
      useApp.getState().setLiveRegister(loadedSettings.liveRegister);
      // Every window loads settings independently (§ shell.ts) — this is what
      // stops the capture panel opening on the store's own default instead of
      // whatever the main window already shows.
      useApp.getState().setTheme(loadedSettings.theme);
      await Promise.all([useApp.getState().loadCorpus(), useApp.getState().loadTypes()]);
    })();
  }, []);

  // Enrichment lands after capture returns, so the canvas has to be told the
  // row changed. Canvas only: the panel does not render titles or questions.
  //
  // Both ends are listened for, not just the landing: the pass takes seconds,
  // and the settled event fires even when it failed, so the indicator clears
  // on the paths where nothing arrives.
  useEffect(() => {
    if (isPanel !== false) return;
    const stops: Array<() => void> = [];
    void (async () => {
      await initBridge();
      const bridge = getBridge();
      stops.push(bridge.onEntryEnriching((entryId) => {
        useApp.getState().setEnriching(entryId, true);
      }));
      stops.push(bridge.onEntryEnriched((entryId) => {
        useApp.getState().setEnriching(entryId, false);
        void useApp.getState().refreshEntry(entryId);
      }));
    })();
    return () => {
      for (const stop of stops) stop();
    };
  }, [isPanel]);

  // Rust routes the shortcut to whichever window should own the recording: the
  // canvas while it has focus, the panel every other time. Both windows listen;
  // only one is ever told, so one press is always one take.
  useEffect(() => {
    if (isPanel === null) return;
    return onHotkey(() => {
      void (async () => {
        // The shortcut is global, so a press can land before this webview has
        // finished starting. initBridge is idempotent; without the await the
        // first press after launch throws on an uninitialised bridge and the
        // recording silently never begins.
        await initBridge();
        const state = useApp.getState();
        if (state.captureState === 'recording') {
          void state.stopRecording();
          return;
        }
        if (state.captureState !== 'idle') return;
        // In the canvas, an open entry makes the hotkey mean "respond to this".
        // The panel has no such context.
        const target = state.selectedEntryId;
        const onNote = state.overlay === 'entry' || state.overlay === 'file';
        const answering = !isPanel && onNote && target ? target : null;
        void state.startRecording(answering);
      })();
    });
  }, [isPanel]);

  // Rust only keeps this shortcut registered for the lifetime of one recording
  // (see src-tauri/src/shortcuts.rs), so it is always safe to read as "discard
  // the recording" without re-checking capture state against a race. Same
  // both-windows routing as the hotkey itself, since discard can land in
  // either one depending on who owns the take.
  useEffect(() => {
    if (isPanel === null) return;
    return onDiscardHotkey(() => {
      const state = useApp.getState();
      if (state.captureState === 'recording') void state.discardRecording();
    });
  }, [isPanel]);

  // The panel window is on screen exactly while capture is running, and never
  // a moment either side of it. Deriving visibility from the state beats asking
  // every exit path to remember: discard has no hand-off to ride out on, and a
  // recording that fails to start would otherwise strand an empty transparent
  // window in the middle of the screen, eating clicks.
  useEffect(() => {
    if (isPanel !== true) return;
    void (captureState === 'idle' ? hidePanel() : showPanel());
  }, [isPanel, captureState]);

  // The canvas takes delivery of what the panel caught, and does nothing else
  // with it. It arrived while you were reading something else, so it waits on
  // the canvas until you come looking rather than interrupting to be read.
  //
  // The entry travels in the event rather than being re-fetched: the panel's
  // bridge is the one that has it.
  useEffect(() => {
    if (isPanel !== false) return;
    return onHandOff(({ entry, question }) => {
      const state = useApp.getState();
      state.upsertEntry(entry);
      if (question) state.addQuestion(entry.id, question);
    });
  }, [isPanel]);

  // Both windows: Settings changed in the main window while the other (usually
  // the panel) is already open. It repaints and relabels its keys now rather
  // than on its next launch.
  useEffect(
    () =>
      onSettingsChange((next) => {
        setSettings(next);
        useApp.getState().setTheme(next.theme);
      }),
    [],
  );

  useEffect(() => {
    if (!settings) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const state = useApp.getState();
      // Same hotkey starts and stops; a separate key discards mid-recording (§4).
      // Under Tauri the global shortcut already does this and drives the panel
      // window, so binding it here too would start two recordings on one press.
      if (!isTauri() && matchesHotkey(e, settings.hotkey)) {
        e.preventDefault();
        if (state.captureState === 'recording') void state.stopRecording();
        else if (state.captureState === 'idle') {
          // An open entry makes the hotkey mean "respond to this" — the answer
          // becomes its own note, joined to it. An unanswered question is what
          // the response closes, not what permits it: an entry you have already
          // answered is exactly the one you come back to months later.
          const target = state.selectedEntryId;
          const onNote = state.overlay === 'entry' || state.overlay === 'file';
          const answering = onNote && target ? target : null;
          void state.startRecording(answering);
        }
        return;
      }
      // Discard honours the configured key; Escape always closes overlays, so
      // rebinding discard never strands you in an open panel.
      if (state.captureState === 'recording' && matchesHotkey(e, settings.discardHotkey)) {
        void state.discardRecording();
        return;
      }
      // C on an open entry starts a link without hover or aim — the handle's
      // pointer-free twin (§5.4).
      const el = e.target as HTMLElement | null;
      const typing = !!el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
      // Both of the single-key shortcuts below act on the canvas, and `typing`
      // only excludes text controls -- a focused row in the list is neither,
      // so tabbing through notes and pressing `f` moved the field underneath.
      const onCanvas = state.view === 'canvas';
      if (
        e.key.toLowerCase() === 'c' && !e.ctrlKey && !e.metaKey && !e.altKey && !typing && onCanvas &&
        state.overlay === 'entry' && state.selectedEntryId && !state.connectSource
      ) {
        e.preventDefault();
        state.setConnectSource(state.selectedEntryId);
        return;
      }
      // F brings every note on screen — the only way back for one stranded
      // past the edge, since §5.1 rules out re-laying out to rescue it.
      if (e.key.toLowerCase() === 'f' && !e.ctrlKey && !e.metaKey && !e.altKey && !typing && onCanvas) {
        e.preventDefault();
        state.fitAll();
        return;
      }
      if (e.key === 'Escape') {
        // Capture first, and in both windows. Mid-capture the only thing
        // escape can plausibly mean is "not this one" -- an overlay behind a
        // recording is not what the key is reaching for.
        if (state.captureState === 'recording' || state.captureState === 'transcribing') {
          e.preventDefault();
          void state.cancelCapture();
          return;
        }
        if (state.connectSource) state.setConnectSource(null);
        else if (state.composing) state.setComposing(false);
        // The file was opened from the note, so escape goes back to the note
        // rather than past it to the canvas.
        else if (state.overlay === 'file') state.setOverlay('entry');
        else if (state.overlay !== 'none') state.closeOverlay();
        else state.dismissPanel();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [settings]);

  // Which window this is decides the whole render, so paint nothing until the
  // answer is known rather than flashing the wrong one.
  if (isPanel === null) return null;

  if (outsideShell) {
    return (
      <main className={styles.main}>
        <p style={{ margin: 'auto', color: 'var(--meta)' }}>
          Parallax runs as a desktop app. Start it with <code>npm run tauri:dev</code>.
        </p>
      </main>
    );
  }

  if (isPanel) {
    return (
      <main className={styles.panelWindow}>
        <CapturePanel settings={settings} />
      </main>
    );
  }

  // Nothing until setup is known, or an installed app flashes onboarding for a
  // frame before opening on the notes.
  if (!isPanel && (settings === null || needsOnboarding === null)) return null;
  if (settings && needsOnboarding && !onboarded) {
    return (
      <main className={styles.main}>
        <Onboarding
          settings={settings}
          onDone={(next) => {
            setSettings(next);
            setOnboarded(true);
          }}
        />
      </main>
    );
  }

  return (
    <main className={styles.main}>
      <TopBar />

      {view === 'list' ? (
        <div className={styles.listArea}>
          {/* An empty corpus gets the empty state in either view: it is the only
              place the sample is offered, and the list is now the way in. The
              list's own empty copy is for a filter that matched nothing. */}
          {loaded && !hasEntries && settings ? (
            <EmptyState hotkey={settings.hotkey} />
          ) : (
            <>
              <Sidebar filter={roleFilter} onFilterChange={setRoleFilter} />
              <ListView filter={roleFilter} />
            </>
          )}
        </div>
      ) : null}

      <div
        className={`${styles.canvasArea} ${view === 'canvas' ? '' : styles.offstage}`}
        aria-hidden={view !== 'canvas'}
      >
        <Canvas />
        {loaded && !hasEntries && settings && view === 'canvas' && (
          <EmptyState hotkey={settings.hotkey} />
        )}
        {loaded && hasEntries && view === 'canvas' && <SparseNotice />}

      </div>

      {/* Outside the canvas container on purpose. Hiding that container to show
          the list also collapsed everything inside it, so opening a note from a
          list row produced a sheet with no width and no height. An overlay
          belongs to the window, not to whichever view is underneath it. */}
      {overlay === 'entry' && settings && (
        <EntryView hotkey={settings.hotkey} liveRegister={settings.liveRegister} />
      )}
      {overlay === 'file' && <FileView />}
      {overlay === 'tasks' && <TaskList />}
      {overlay === 'settings' && settings && (
        <SettingsPanel
          settings={settings}
          onChange={(next) => {
            setSettings(next);
            useApp.getState().setLiveRegister(next.liveRegister);
            useApp.getState().setTheme(next.theme);
            void broadcastSettings(next);
          }}
        />
      )}
      <CapturePanel settings={settings} />
      <ConnectPicker />
      <RelationPicker />

      {/* Mounted rather than conditional, so a question and its results survive
          being closed and reopened -- recall you have to retype is not recall. */}
      <ChatPanel />

      {composing && <TypedComposer onClose={() => setComposing(false)} />}

      <PlayerPill />

      <div className={styles.bottomRow}>
        {settings && <StatusBar settings={settings} />}
        {/* Not gated on having entries: upload is how a corpus arrives, and
            hiding the pill until one exists left no way to import into an empty
            app. Everything in it that needs entries disables itself. */}
        {loaded && <ActionPill />}
      </div>
    </main>
  );
}
