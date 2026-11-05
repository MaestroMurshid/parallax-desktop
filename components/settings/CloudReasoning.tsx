'use client';

import { useEffect, useState } from 'react';
import { getBridge, type ApiKeyStatus, type CloudProvider } from '@/lib/bridge';
import type { Settings } from '@/lib/types';
import styles from './SettingsPanel.module.css';

const reason = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** Who the words go to, named the way the person would recognise it. */
function destination(settings: Settings): string {
  if (settings.reasoningProvider === 'anthropic') return 'Anthropic';
  try {
    return new URL(settings.openaiBaseUrl).hostname;
  } catch {
    return 'the provider';
  }
}

/**
 * A text setting that commits on Enter or on leaving the field, not on every
 * keystroke — each commit is a settings write, and a half-typed endpoint is
 * refused by the backend before it is finished being typed.
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
  useEffect(() => setDraft(value), [value]);

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
      {error && <p className={styles.reject}>{error}</p>}
    </>
  );
}

/**
 * The reasoning model on someone else's computer, with the user's own key.
 *
 * Everything the trade costs is said here, next to the switch, rather than in
 * a dialog: what leaves (the text of notes), what never does (audio,
 * transcription, embeddings, and any note marked local-only), and where it
 * goes. The key is write-only from this side — saved to the OS keychain and
 * never read back into the window.
 */
export default function CloudReasoning({
  settings,
  update,
}: {
  settings: Settings;
  update(patch: Partial<Settings>): Promise<void>;
}) {
  const provider = settings.reasoningProvider as CloudProvider;
  const [status, setStatus] = useState<ApiKeyStatus | null>(null);
  const [replacing, setReplacing] = useState(false);
  const [draft, setDraft] = useState('');
  const [keyError, setKeyError] = useState<string | null>(null);
  const [check, setCheck] = useState<
    { state: 'idle' } | { state: 'checking' } | { state: 'ok' | 'failed'; message: string }
  >({ state: 'idle' });

  const refresh = () =>
    getBridge()
      .apiKeyStatus()
      .then(setStatus)
      .catch(() => setStatus({ anthropic: false, openai: false }));

  useEffect(() => {
    void refresh();
  }, []);

  // A different provider is a different key and a different answer.
  useEffect(() => {
    setDraft('');
    setReplacing(false);
    setKeyError(null);
    setCheck({ state: 'idle' });
  }, [provider]);

  const saved = status?.[provider] ?? false;
  const editingKey = !saved || replacing;

  const saveKey = async () => {
    if (!draft.trim()) return;
    try {
      await getBridge().setApiKey(provider, draft);
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
      await getBridge().clearApiKey(provider);
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
        <span className={styles.label}>API key</span>
        <div className={styles.control}>
          {editingKey ? (
            <>
              <input
                type="password"
                className={styles.field}
                aria-label={`${destination(settings)} API key`}
                value={draft}
                autoComplete="off"
                spellCheck={false}
                placeholder={provider === 'anthropic' ? 'sk-ant-…' : 'paste the key'}
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
          {provider === 'anthropic' ? (
            <CommitField
              label="Anthropic model"
              value={settings.anthropicModel}
              placeholder="claude-opus-5-5"
              onCommit={(anthropicModel) => update({ anthropicModel })}
            />
          ) : (
            <CommitField
              label="Model name"
              value={settings.openaiModel}
              placeholder="the model id your provider lists"
              onCommit={(openaiModel) => update({ openaiModel })}
            />
          )}
        </div>
      </div>

      {provider === 'openai' && (
        <>
          <div className={styles.row}>
            <span className={styles.label}>endpoint</span>
            <div className={styles.control}>
              <CommitField
                label="Endpoint"
                value={settings.openaiBaseUrl}
                placeholder="https://api.openai.com/v1"
                onCommit={(openaiBaseUrl) => update({ openaiBaseUrl })}
              />
            </div>
          </div>
          <p className={styles.helper}>
            Any OpenAI-compatible API: OpenAI, OpenRouter, Groq, or a server on this computer.
            The model has to support structured (JSON schema) output.
          </p>
        </>
      )}

      <div className={styles.row}>
        <span className={styles.label}>check</span>
        <div className={styles.control}>
          <button
            type="button"
            className={styles.inline}
            disabled={!saved || check.state === 'checking'}
            onClick={() => void runCheck()}
          >
            {check.state === 'checking' ? 'asking…' : 'send a test message'}
          </button>
          {check.state === 'ok' && <span className={styles.ready}>{check.message}</span>}
        </div>
      </div>
      {check.state === 'failed' && <p className={styles.reject}>{check.message}</p>}

      {/* Fact, not a setting, like the transcription one above. */}
      <p className={styles.statement}>
        Filing, questions, connections and answers in recall send the text of your notes to{' '}
        {destination(settings)}, billed to your key. Audio, transcription and the search itself stay
        on this machine, and a note marked local-only is never sent.
      </p>

      <div className={styles.row}>
        <span className={styles.label}>new notes</span>
        <div className={styles.control}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={settings.defaultLocalOnly}
              onChange={(e) => void update({ defaultLocalOnly: e.target.checked })}
            />
            <span>mark local-only (never sent to a cloud model)</span>
          </label>
        </div>
      </div>
      {settings.defaultLocalOnly && (
        <p className={styles.helper}>
          New notes are kept as written and not filed or questioned while a cloud model is chosen.
        </p>
      )}
    </>
  );
}
