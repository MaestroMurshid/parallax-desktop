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
import Onboarding from '@/components/onboarding/Onboarding';
import CapturePanel from '@/components/panel/CapturePanel';
import TypedComposer from '@/components/panel/TypedComposer';
import SettingsPanel from '@/components/settings/SettingsPanel';
import TaskList from '@/components/tasks/TaskList';
import { getBridge, initBridge, isTauri } from '@/lib/bridge';
import { hidePanel, isPanelWindow, onHandOff, onHotkey, showPanel } from '@/lib/shell';
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
  // The capture panel is its own borderless always-on-top Tauri window (§4),
  // pointed at this same page. Same markup, no second route (§9.1). Null until
  // mount, because a static export has no window to ask at prerender time and
  // guessing wrong paints the canvas inside the panel for a frame.
  const [isPanel, setIsPanel] = useState<boolean | null>(null);
  const loaded = useApp((s) => s.loaded);
  const hasEntries = useApp((s) => s.order.length > 0);
  const overlay = useApp((s) => s.overlay);
  const composing = useApp((s) => s.composing);
  const captureState = useApp((s) => s.captureState);
  const setComposing = useApp((s) => s.setComposing);

  useEffect(() => {
    const panel = isPanelWindow();
    setIsPanel(panel);
    // Lets the stylesheet drop the page surface for this window; the panel is
    // meant to float over other apps, not to be a grey box on the desktop.
    if (panel) document.documentElement.dataset.window = 'panel';
    void (async () => {
      await initBridge();
      setSettings(await getBridge().getSettings());
      await useApp.getState().loadCorpus();
    })();
  }, []);

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
        const answering = !isPanel && state.overlay === 'entry' && target ? target : null;
        void state.startRecording(answering);
      })();
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
  // bridge is the one that has it, and under the fixture backend each window
  // keeps its own corpus in memory.
  useEffect(() => {
    if (isPanel !== false) return;
    return onHandOff(({ entry, question }) => {
      const state = useApp.getState();
      state.upsertEntry(entry);
      if (question) state.addQuestion(entry.id, question);
    });
  }, [isPanel]);

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
          const answering = state.overlay === 'entry' && target ? target : null;
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
      if (
        e.key.toLowerCase() === 'c' && !e.ctrlKey && !e.metaKey && !e.altKey && !typing &&
        state.overlay === 'entry' && state.selectedEntryId && !state.connectSource
      ) {
        e.preventDefault();
        state.setConnectSource(state.selectedEntryId);
        return;
      }
      if (e.key === 'Escape') {
        if (state.connectSource) state.setConnectSource(null);
        else if (state.composing) state.setComposing(false);
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

  if (isPanel) {
    return (
      <main className={styles.panelWindow}>
        <CapturePanel />
      </main>
    );
  }

  // Mockup flag: replay onboarding on every load regardless of saved settings.
  // Real behaviour is "first run only" — drop NEXT_PUBLIC_ALWAYS_ONBOARD to get it.
  const alwaysOnboard = process.env.NEXT_PUBLIC_ALWAYS_ONBOARD === '1';
  if (settings && (alwaysOnboard ? !onboarded : settings.modelId === null)) {
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

      <div className={styles.canvasArea}>
        <Canvas />
        {loaded && !hasEntries && settings && <EmptyState hotkey={settings.hotkey} />}
        {loaded && hasEntries && <SparseNotice />}

        {overlay === 'entry' && settings && <EntryView hotkey={settings.hotkey} />}
        {overlay === 'tasks' && <TaskList />}
        {overlay === 'settings' && settings && <SettingsPanel settings={settings} onChange={setSettings} />}
        <CapturePanel />
        <ConnectPicker />
        <RelationPicker />
      </div>

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
