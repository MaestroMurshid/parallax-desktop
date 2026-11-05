'use client';

import { useEffect, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import type { Settings } from '@/lib/types';
import styles from './SettingsPanel.module.css';

const reason = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/**
 * Shortcuts that fill the URL (and a model, where one is a safe default).
 * Nothing about them is special: any URL works, and these are only the ones
 * worth not having to look up. api.anthropic.com is spoken to as Anthropic's
 * own API; every other URL as OpenAI-style chat completions.
 */
const PRESETS: ReadonlyArray<{ name: string; url: string; model?: string }> = [
  { name: 'Anthropic', url: 'https://api.anthropic.com/v1', model: 'claude-opus-5-5' },
  { name: 'OpenAI', url: 'https://api.openai.com/v1' },
  { name: 'OpenRouter', url: 'https://openrouter.ai/api/v1' },
  { name: 'Groq', url: 'https://api.groq.com/openai/v1' },
  { name: 'Ollama', url: 'http://localhost:11434/v1' },
  { name: 'LM Studio', url: 'http://localhost:1234/v1' },
];

function parse(url: string): URL | null {
  try {
    return new URL(url);
  } catch {
    return null;
  }
}

/** Mirrors `Endpoint::on_this_machine` in Rust, which is the one that counts. */
function onThisMachine(url: URL | null): boolean {
  if (!url) return false;
  return ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname.toLowerCase());
}

/**
 * A text setting that commits on Enter or on leaving the field, not on every
 * keystroke — each commit is a settings write, and a half-typed URL is refused
 * by the backend before it is finished being typed.
 */
function CommitField({
  value,
  placeholder,
  label,
  onCommit,
}: {
  value: string;
  placeholder?: string;
  label: string;
  onCommit(next: string): Promise<void>;
}) {
  const [draft, setDraft] = useState(value);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setDraft(value);
    setError(null);
  }, [value]);

  const commit = async () => {
    const next = draft.trim();
    if (next === value) return;
    try {
      await onCommit(next);
      setError(null);
    } catch (e) {
      setError(reason(e));
    }
  };

  return (
    <>
      <input
        className={`${styles.field} selectable`}
        aria-label={label}
        value={draft}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void commit();
          // Escape belongs to the sheet; here it only puts the field back.
          if (e.key === 'Escape') {
            e.stopPropagation();
            setDraft(value);
          }
        }}
      />
      {error && <span className={styles.reject}>{error}</span>}
    </>
  );
}

/**
 * Reasoning through an API, picked by URL alone.
 *
 * Everything the trade costs is said here, next to the switch, rather than in
 * a dialog: what leaves (the text of notes), what never does (audio,
 * transcription, embeddings, and any note marked local-only), and where it
 * goes. The key is write-only from this side — saved to the OS keychain under
 * the URL's host and never read back into the window.
 */
export default function CloudReasoning({
  settings,
  update,
}: {
  settings: Settings;
  update(patch: Partial<Settings>): Promise<void>;
}) {
  const url = settings.apiBaseUrl;
  const parsed = parse(url);
  const local = onThisMachine(parsed);
  const host = parsed?.host ?? 'the API';

  const [saved, setSaved] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [draft, setDraft] = useState('');
  const [keyError, setKeyError] = useState<string | null>(null);
  const [check, setCheck] = useState<
    { state: 'idle' } | { state: 'checking' } | { state: 'ok' | 'failed'; message: string }
  >({ state: 'idle' });

  const refresh = () =>
    getBridge()
      .apiKeySaved(url)
      .then(setSaved)
      .catch(() => setSaved(false));

  // Keys are filed by host, so a different URL may well have its own key
  // already -- or none -- and the last check said nothing about this one.
  useEffect(() => {
    void refresh();
    setDraft('');
    setReplacing(false);
    setKeyError(null);
    setCheck({ state: 'idle' });
  }, [url]);

  const editingKey = !saved || replacing;
  const ready = settings.apiModel.trim() !== '' && (saved || local);

  const saveKey = async () => {
    if (!draft.trim()) return;
    try {
      await getBridge().setApiKey(url, draft);
      setDraft('');
      setReplacing(false);
      setKeyError(null);
      setCheck({ state: 'idle' });
      await refresh();
    } catch (e) {
      setKeyError(reason(e));
    }
  };

  const removeKey = async () => {
    try {
      await getBridge().clearApiKey(url);
      setCheck({ state: 'idle' });
      await refresh();
    } catch (e) {
      setKeyError(reason(e));
    }
  };

  const runCheck = async () => {
    setCheck({ state: 'checking' });
    try {
      const who = await getBridge().testReasoning();
      setCheck({ state: 'ok', message: who });
    } catch (e) {
      setCheck({ state: 'failed', message: reason(e) });
    }
  };

  return (
    <>
      <div className={styles.row}>
        <span className={styles.label}>URL</span>
        <div className={styles.control}>
          <CommitField
            label="API URL"
            value={url}
            placeholder="https://…/v1"
            onCommit={(apiBaseUrl) => update({ apiBaseUrl })}
          />
        </div>
      </div>
      <div className={styles.row}>
        <span className={styles.label} />
        <div className={styles.options}>
          {PRESETS.map((p) => (
            <button
              key={p.name}
              type="button"
              className={p.url === url ? styles.optionOn : styles.option}
              // A model from one service means nothing to another, so a preset
              // without a safe default clears it rather than keeping a stale one.
              onClick={() => void update({ apiBaseUrl: p.url, apiModel: p.model ?? '' })}
            >
              {p.name}
            </button>
          ))}
        </div>
      </div>
      <p className={styles.helper}>
        Any OpenAI-compatible URL works, as does Anthropic&rsquo;s. The model has to support
        structured (JSON schema) output.
      </p>

      <div className={styles.row}>
        <span className={styles.label}>API key</span>
        <div className={styles.control}>
          {editingKey ? (
            <>
              <input
                type="password"
                className={styles.field}
                aria-label={`API key for ${host}`}
                value={draft}
                autoComplete="off"
                spellCheck={false}
                placeholder={local ? 'usually none needed' : `key for ${host}`}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void saveKey();
                }}
              />
              <button
                type="button"
                className={styles.inline}
                onClick={() => void saveKey()}
                disabled={!draft.trim()}
              >
                save
              </button>
              {replacing && (
                <button type="button" className={styles.inline} onClick={() => setReplacing(false)}>
                  cancel
                </button>
              )}
            </>
          ) : (
            <>
              <span className={styles.value}>saved in the system keychain</span>
              <button type="button" className={styles.inline} onClick={() => setReplacing(true)}>
                replace
              </button>
              <button type="button" className={styles.inline} onClick={() => void removeKey()}>
                remove
              </button>
            </>
          )}
        </div>
      </div>
      {keyError && <p className={styles.reject}>{keyError}</p>}

      <div className={styles.row}>
        <span className={styles.label}>model</span>
        <div className={styles.control}>
          <CommitField
            label="Model"
            value={settings.apiModel}
            placeholder="the model id the service lists"
            onCommit={(apiModel) => update({ apiModel })}
          />
        </div>
      </div>

      <div className={styles.row}>
        <span className={styles.label}>check</span>
        <div className={styles.control}>
          <button
            type="button"
            className={styles.inline}
            disabled={!ready || check.state === 'checking'}
            onClick={() => void runCheck()}
          >
            {check.state === 'checking' ? 'asking…' : 'send a test message'}
          </button>
          {check.state === 'ok' && <span className={styles.ready}>{check.message}</span>}
        </div>
      </div>
      {check.state === 'failed' && <p className={styles.reject}>{check.message}</p>}

      {/* Fact, not a setting, like the transcription one above. */}
      {local ? (
        <p className={styles.statement}>
          The server at {host} is on this computer, so your notes stay on this machine.
        </p>
      ) : (
        <p className={styles.statement}>
          Filing, questions, connections and answers in recall send the text of your notes to{' '}
          {host}, billed to your key. Audio, transcription and the search itself stay on this
          machine, and a note marked local-only is never sent.
        </p>
      )}

      {!local && (
        <>
          <div className={styles.row}>
            <span className={styles.label}>new notes</span>
            <div className={styles.control}>
              <label className={styles.toggle}>
                <input
                  type="checkbox"
                  checked={settings.defaultLocalOnly}
                  onChange={(e) => void update({ defaultLocalOnly: e.target.checked })}
                />
                <span>mark local-only (never sent to an API)</span>
              </label>
            </div>
          </div>
          {settings.defaultLocalOnly && (
            <p className={styles.helper}>
              New notes are kept as written, and not filed or questioned, while an API off this
              machine is chosen.
            </p>
          )}
        </>
      )}
    </>
  );
}
