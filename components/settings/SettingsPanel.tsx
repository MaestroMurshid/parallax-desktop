'use client';

import { useEffect, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import type { ModelInfo, Settings } from '@/lib/types';
import styles from './SettingsPanel.module.css';
import TypeEditor from './TypeEditor';

const gb = (bytes: number) => `${(bytes / 1e9).toFixed(1)}GB`;


type Rebindable = 'hotkey' | 'discardHotkey';

/**
 * Right-hand sheet, same pattern as EntryView: fixed, own background, esc to
 * close (handled by the app-wide overlay Escape handling in app/page.tsx).
 */
export default function SettingsPanel({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange(next: Settings): void;
}) {
  const close = useApp((s) => s.closeOverlay);
  const theme = useApp((s) => s.theme);
  const setTheme = useApp((s) => s.setTheme);
  const entryCount = useApp((s) => s.order.length);
  const loadSample = useApp((s) => s.loadSample);
  const clearSample = useApp((s) => s.clearSample);
  const setSampleLoaded = useApp((s) => s.setSampleLoaded);

  const [models, setModels] = useState<ModelInfo[]>([]);
  const [capturing, setCapturing] = useState<Rebindable | null>(null);

  useEffect(() => {
    const bridge = getBridge();
    void bridge.listModels().then(setModels);
    return bridge.onModelProgress((m) => {
      setModels((prev) => prev.map((x) => (x.id === m.id ? m : x)));
    });
  }, []);

  // Capture the next chord for whichever field is rebinding — same logic as
  // onboarding's hotkey capture, generalised to either field here.
  useEffect(() => {
    if (!capturing) return;
    const field = capturing;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
      const parts = [
        e.ctrlKey && 'Ctrl',
        e.shiftKey && 'Shift',
        e.altKey && 'Alt',
        e.code === 'Space' ? 'Space' : e.key.toUpperCase(),
      ].filter(Boolean);
      const chord = parts.join('+');
      void update(field === 'hotkey' ? { hotkey: chord } : { discardHotkey: chord });
      setCapturing(null);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [capturing]);

  async function update(patch: Partial<Settings>) {
    const next = await getBridge().setSettings(patch);
    onChange(next);
  }

  return (
    <aside className={styles.sheet}>
      <header className={styles.header}>
        <span className={styles.title}>settings</span>
        <button type="button" className={styles.close} onClick={close} aria-label="Close">
          esc
        </button>
      </header>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>capture</h2>

        <div className={styles.row}>
          <span className={styles.label}>record hotkey</span>
          <div className={styles.control}>
            <kbd className={styles.kbd}>
              {capturing === 'hotkey' ? 'press a combination…' : settings.hotkey}
            </kbd>
            <button type="button" className={styles.inline} onClick={() => setCapturing('hotkey')}>
              rebind
            </button>
          </div>
        </div>

        <div className={styles.row}>
          <span className={styles.label}>discard key</span>
          <div className={styles.control}>
            <kbd className={styles.kbd}>
              {capturing === 'discardHotkey' ? 'press a combination…' : settings.discardHotkey}
            </kbd>
            <button
              type="button"
              className={styles.inline}
              onClick={() => setCapturing('discardHotkey')}
            >
              rebind
            </button>
          </div>
        </div>

        <div className={styles.row}>
          <span className={styles.label}>local-only</span>
          <div className={styles.control}>
            <label className={styles.toggle}>
              <input
                type="checkbox"
                checked={settings.defaultLocalOnly}
                onChange={(e) => void update({ defaultLocalOnly: e.target.checked })}
              />
              <span>new entries default to local-only</span>
            </label>
          </div>
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>transcription</h2>

        <div className={styles.row}>
          <span className={styles.label}>model</span>
          <div className={styles.options}>
            {models
              .filter((m) => m.kind === 'transcription')
              .map((m) => (
                <button
                  key={m.id}
                  type="button"
                  className={
                    m.name === settings.transcriptionModel ? styles.optionOn : styles.option
                  }
                  onClick={() =>
                    void update({
                      transcriptionModel: m.name as Settings['transcriptionModel'],
                    })
                  }
                >
                  {m.name} · {Math.round(m.sizeBytes / 1e6)}MB
                  {m.state.kind === 'ready' ? ' · ready' : ''}
                </button>
              ))}
          </div>
        </div>

        {/* Fact, not a setting — no control here on purpose. */}
        <p className={styles.statement}>Audio and transcription never leave this machine.</p>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>reasoning model</h2>

        <div className={styles.models}>
          {models
            .filter((m) => m.kind === 'reasoning')
            .map((model) => (
            <div key={model.id} className={model.id === settings.modelId ? styles.modelRowOn : styles.modelRow}>
              <button
                type="button"
                className={styles.modelSelect}
                onClick={() => void update({ modelId: model.id })}
              >
                <span className={styles.modelName}>{model.name}</span>
                <span className={styles.modelMeta}>
                  {model.quantization} · {gb(model.sizeBytes)}
                </span>
              </button>

              <div className={styles.modelState}>
                {model.state.kind === 'ready' && <span className={styles.ready}>ready</span>}
                {model.state.kind === 'failed' && <span className={styles.failed}>failed</span>}
                {model.state.kind === 'not-downloaded' && (
                  <button
                    type="button"
                    className={styles.download}
                    onClick={() => void getBridge().downloadModel(model.id)}
                  >
                    download
                  </button>
                )}
                {model.state.kind === 'downloading' && (
                  <div className={styles.progressTrack} aria-hidden>
                    <div
                      className={styles.progressFill}
                      style={{
                        width: `${Math.round((model.state.receivedBytes / model.state.totalBytes) * 100)}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>

        <div className={styles.row}>
          <span className={styles.label}>residency</span>
          <div className={styles.options}>
            {(['warm', 'cold'] as const).map((r) => (
              <button
                key={r}
                type="button"
                className={r === settings.residency ? styles.optionOn : styles.option}
                onClick={() => void update({ residency: r })}
              >
                {r}
              </button>
            ))}
          </div>
        </div>
        <p className={styles.helper}>
          Warm answers in ~2s but holds 1–4.5GB in memory. Cold frees the RAM but takes 10s+.
        </p>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>appearance</h2>
        <div className={styles.row}>
          <span className={styles.label}>theme</span>
          <div className={styles.options}>
            {(['system', 'light', 'dark'] as const).map((t) => (
              <button
                key={t}
                type="button"
                className={t === theme ? styles.optionOn : styles.option}
                onClick={() => setTheme(t)}
              >
                {t}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>types</h2>
        <TypeEditor />
        <p className={styles.helper}>
          A type you add cannot fire on felt or inert entries, or on anything under 30 seconds.
          The mark is yours; the gate is not.
        </p>
      </section>

      <section className={styles.section}>
        <h2 className={styles.sectionTitle}>corpus</h2>

        <div className={styles.row}>
          <span className={styles.label}>entries</span>
          <span className={styles.value}>{entryCount}</span>
        </div>

        <div className={styles.actions}>
          <button
            type="button"
            className={styles.inline}
            onClick={async () => {
              await loadSample();
              setSampleLoaded(true);
            }}
          >
            load sample corpus
          </button>
          <button
            type="button"
            className={styles.inline}
            onClick={async () => {
              await clearSample();
              setSampleLoaded(false);
            }}
          >
            clear sample corpus
          </button>
        </div>
      </section>
    </aside>
  );
}
