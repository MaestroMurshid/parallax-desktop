'use client';

import { useEffect, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { APP_NAME } from '@/lib/constants';
import { useApp } from '@/lib/store';
import type { ModelInfo, Residency, Settings, SystemProfile } from '@/lib/types';
import MarkGlyph from '@/components/canvas/MarkGlyph';
import styles from './Onboarding.module.css';

const gb = (bytes: number) => `${(bytes / 1e9).toFixed(1)}GB`;
const mb = (bytes: number) => `${Math.round(bytes / 1e6)}MB`;
const size = (m: ModelInfo) => (m.sizeBytes >= 1e9 ? gb(m.sizeBytes) : mb(m.sizeBytes));

/** Largest model the machine comfortably clears; the default, not a question. */
function recommend(models: ModelInfo[], profile: SystemProfile): ModelInfo | undefined {
  const affordable = models.filter((m) => profile.totalRamBytes >= m.recommendedRamBytes);
  return affordable[affordable.length - 1] ?? models[0];
}

function progressOf(model: ModelInfo | undefined): { label: string; pct: number } {
  if (!model) return { label: '', pct: 0 };
  switch (model.state.kind) {
    case 'downloading': {
      const pct = Math.round((model.state.receivedBytes / model.state.totalBytes) * 100);
      return { label: `${model.name} · ${pct}%`, pct };
    }
    case 'ready':
      return { label: `${model.name} · ready`, pct: 100 };
    case 'failed':
      return { label: `${model.name} · failed`, pct: 0 };
    default:
      return { label: `${model.name} · queued`, pct: 0 };
  }
}

/**
 * Two beats, not a wizard. Models are chosen first only so their downloads run
 * while you set everything else up. Transcription is the one that gates
 * recording; the reasoning model can land late and the question waits (§9.4).
 */
export default function Onboarding({
  settings,
  onDone,
}: {
  settings: Settings;
  onDone(next: Settings): void;
}) {
  const [step, setStep] = useState<'models' | 'field' | 'types' | 'links'>('models');
  const theme = useApp((s) => s.theme);
  const setTheme = useApp((s) => s.setTheme);
  const [profile, setProfile] = useState<SystemProfile | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelId, setModelId] = useState<string | null>(null);
  const [transcriptionId, setTranscriptionId] = useState<string | null>(null);
  const [hotkey, setHotkey] = useState(settings.hotkey);
  const [residency, setResidency] = useState<Residency>(settings.residency);
  const [advanced, setAdvanced] = useState(false);
  const [micGranted, setMicGranted] = useState(false);
  const [capturing, setCapturing] = useState(false);

  useEffect(() => {
    void (async () => {
      const bridge = getBridge();
      const [p, m] = await Promise.all([bridge.getSystemProfile(), bridge.listModels()]);
      setProfile(p);
      setModels(m);
      setModelId(recommend(m.filter((x) => x.kind === 'reasoning'), p)?.id ?? null);
      const speech = m.filter((x) => x.kind === 'transcription');
      setTranscriptionId(
        speech.find((x) => x.name === settings.transcriptionModel)?.id ?? speech[1]?.id ?? null,
      );
    })();
    return getBridge().onModelProgress((m) =>
      setModels((prev) => prev.map((x) => (x.id === m.id ? m : x))),
    );
  }, [settings.transcriptionModel]);

  // Capture the next chord as the hotkey, so it is tested by being set.
  useEffect(() => {
    if (!capturing) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
      const parts = [
        e.ctrlKey && 'Ctrl',
        e.shiftKey && 'Shift',
        e.altKey && 'Alt',
        e.code === 'Space' ? 'Space' : e.key.toUpperCase(),
      ].filter(Boolean);
      setHotkey(parts.join('+'));
      setCapturing(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [capturing]);

  const speechModels = models.filter((m) => m.kind === 'transcription');
  const reasoningModels = models.filter((m) => m.kind === 'reasoning');
  const speech = models.find((m) => m.id === transcriptionId);
  const reasoning = models.find((m) => m.id === modelId);
  const speechProgress = progressOf(speech);
  const reasoningProgress = progressOf(reasoning);

  /** Start both downloads and move on — the point of splitting the two beats. */
  const chooseModels = async () => {
    const bridge = getBridge();
    if (speech) {
      await bridge.setSettings({ transcriptionModel: speech.name as Settings['transcriptionModel'] });
      void bridge.downloadModel(speech.id);
    }
    if (modelId) {
      await bridge.setSettings({ modelId });
      void bridge.downloadModel(modelId);
    }
    setStep('field');
  };

  const start = async () => {
    const next = await getBridge().setSettings({ hotkey, residency, modelId });
    onDone(next);
  };

  // Enter is the default action on both beats.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Enter' || capturing) return;
      if ((e.target as HTMLElement | null)?.tagName === 'BUTTON') return;
      if (step === 'models') return void chooseModels();
      if (step === 'field') return setStep('types');
      if (step === 'types') return setStep('links');
      void start();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [step, capturing, hotkey, modelId, transcriptionId, residency]);

  return (
    <div className={styles.stage}>
      <div className={styles.frame}>
        <div className={styles.topRow}>
          <span className={styles.top}>{step === 'models' ? '' : APP_NAME}</span>
          {/* Here rather than buried in settings: this is the first thing the
              app shows, and it is the screen you would want to change it on. */}
          <div className={styles.themes} role="group" aria-label="Appearance">
            {(['system', 'light', 'dark'] as const).map((t) => (
              <button
                key={t}
                type="button"
                className={theme === t ? styles.themeOn : styles.theme}
                aria-pressed={theme === t}
                onClick={() => setTheme(t)}
              >
                {t}
              </button>
            ))}
          </div>
        </div>

        {step === 'models' && (
          <div className={styles.masthead}>
            <h1 className={styles.wordmark}>{APP_NAME}</h1>
            <p className={styles.tagline}>
              Talk. It files what you said, reasons and connects with you.
            </p>
          </div>
        )}

        {step === 'models' && (
          <div className={styles.rows}>
            <div className={styles.node}>
              <span className={styles.t}>Transcription</span>
              <span className={styles.m}>
                {speech ? `transcribe.cpp ${speech.name} · ${size(speech)}` : 'detecting…'}
                <span className={styles.sep}>·</span>
                <button type="button" className={styles.link} onClick={() => setAdvanced((v) => !v)}>
                  {advanced ? 'hide' : 'change'}
                </button>
              </span>
              <span className={styles.helper}>
                Runs on every recording, so it has to be here before the first one. Small and local.
              </span>
            </div>

            <div className={styles.node}>
              <span className={styles.t}>Reasoning</span>
              <span className={styles.m}>
                {reasoning ? `${reasoning.name} ${reasoning.quantization} · ${size(reasoning)}` : 'detecting…'}
                <span className={styles.sep}>·</span>
                <button type="button" className={styles.link} onClick={() => setAdvanced((v) => !v)}>
                  {advanced ? 'hide' : 'change'}
                </button>
              </span>
              <span className={styles.helper}>
                Only needed for the question, so it can arrive late
                {profile && `. ${gb(profile.totalRamBytes)} RAM · ${profile.cpuCores} cores`}.
              </span>
            </div>

            {advanced && (
              <div className={styles.advanced}>
                <span className={styles.advLabel}>transcription</span>
                <div className={styles.options}>
                  {speechModels.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={m.id === transcriptionId ? styles.optionOn : styles.option}
                      onClick={() => setTranscriptionId(m.id)}
                    >
                      {m.name} · {size(m)}
                    </button>
                  ))}
                </div>
                <span className={styles.advLabel}>reasoning</span>
                <div className={styles.options}>
                  {reasoningModels.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={m.id === modelId ? styles.optionOn : styles.option}
                      onClick={() => setModelId(m.id)}
                    >
                      {m.name} · {size(m)}
                    </button>
                  ))}
                </div>
                <span className={styles.advLabel}>residency</span>
                <div className={styles.options}>
                  {(['warm', 'cold'] as const).map((r) => (
                    <button
                      key={r}
                      type="button"
                      className={r === residency ? styles.optionOn : styles.option}
                      onClick={() => setResidency(r)}
                    >
                      {r === 'warm' ? 'warm · ~2s' : 'cold · frees RAM'}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}

        {step === 'types' && (
          <div className={styles.how}>
            <p className={styles.lede}>What a note can be.</p>

            <div className={styles.kinds}>
              <div className={styles.kind}>
                <span className={styles.kindHead}>
                  <MarkGlyph mark={{ kind: 'glyph', id: 'position' }} size={11} />
                  <span className={`${styles.kindName} ${styles.kPosition}`}>position</span>
                </span>
                <span className={styles.said}>
                  &ldquo;Upbringing, genetics, circumstances. They shape what I want. But I still
                  choose how I respond to it.&rdquo;
                </span>
                <span className={styles.back}>
                  This rests on the responding being separate from what shaped you. Is it separate,
                  or only later?
                </span>
              </div>

              <div className={styles.kind}>
                <span className={styles.kindHead}>
                  <MarkGlyph mark={{ kind: 'glyph', id: 'evidence' }} size={11} />
                  <span className={`${styles.kindName} ${styles.kEvidence}`}>evidence</span>
                </span>
                <span className={styles.said}>
                  &ldquo;The hard problem is why there is something it is like to be you at all,
                  not how the brain processes information.&rdquo;
                </span>
                <span className={styles.back}>
                  You wake up tomorrow unable to feel pain, emotion or pleasure, but you can still
                  think, speak, remember your childhood and solve problems. Would you still call
                  yourself conscious?
                </span>
              </div>

              <div className={styles.kind}>
                <span className={styles.kindHead}>
                  <MarkGlyph mark={{ kind: 'glyph', id: 'note' }} size={11} />
                  <span className={`${styles.kindName} ${styles.kNote}`}>note</span>
                </span>
                <span className={styles.said}>
                  &ldquo;Cancel the storage tier, and move the climate feeds off that reader that
                  got acquired.&rdquo;
                </span>
                <span className={styles.backQuiet}>
                  both items &rarr; task list, nothing asked
                </span>
              </div>
            </div>

          </div>
        )}

        {step === 'links' && (
          <div className={styles.how}>
            <p className={styles.lede}>How notes find each other.</p>

            <div className={styles.pair}>
              <div className={styles.pairSide}>
                <span className={styles.pairDate}>12 Mar</span>
                <span className={styles.pairTitle}>I still choose how I respond</span>
              </div>
              <div className={styles.pairLink} aria-hidden="true">
                <span className={styles.pairRule} />
                <span className={styles.relation}>contradicts</span>
                <span className={styles.gapLabel}>200 days apart</span>
              </div>
              <div className={styles.pairSide}>
                <span className={styles.pairDate}>28 Sep</span>
                <span className={styles.pairTitle}>the choosing is conditioned too</span>
              </div>
            </div>

            <blockquote className={styles.asked}>
              If you never had control over the conditions that shaped you, can you still be
              responsible for what those conditions eventually cause you to do?
            </blockquote>

          </div>
        )}

        {step === 'field' && (
          <div className={styles.canvas}>
            {/* Same hairline weight and colour the canvas uses for a proposed edge. */}
            <svg className={styles.threads} viewBox="0 0 600 300" preserveAspectRatio="none" aria-hidden="true">
              <line x1="95" y1="50" x2="215" y2="148" />
              <line x1="420" y1="74" x2="245" y2="150" />
              <line x1="255" y1="158" x2="400" y2="198" />
            </svg>

            <div className={styles.node} style={{ left: '4%', top: '10%' }}>
              <span className={styles.t}>Hotkey</span>
              <span className={styles.m}>
                <kbd className={styles.kbd}>{capturing ? 'press a combination…' : hotkey}</kbd>
                <span className={styles.sep}>·</span>
                <button type="button" className={styles.link} onClick={() => setCapturing(true)}>
                  rebind
                </button>
              </span>
            </div>

            <div className={styles.node} style={{ left: '66%', top: '18%' }}>
              <span className={styles.t}>Microphone</span>
              <span className={styles.m}>
                microphone
                <span className={styles.sep}>·</span>
                {micGranted ? (
                  'allowed'
                ) : (
                  <button
                    type="button"
                    className={styles.link}
                    onClick={async () => {
                      try {
                        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                        stream.getTracks().forEach((t) => t.stop());
                        setMicGranted(true);
                      } catch {
                        setMicGranted(false);
                      }
                    }}
                  >
                    allow
                  </button>
                )}
              </span>
            </div>

            <div className={styles.node} style={{ left: '30%', top: '44%' }}>
              <span className={styles.t}>Transcription</span>
              <span className={styles.m}>{speechProgress.label || 'arriving'}</span>
            </div>

            <div className={styles.node} style={{ left: '62%', top: '62%' }}>
              <span className={styles.t}>Reasoning</span>
              <span className={styles.m}>{reasoningProgress.label || 'arriving'}</span>
            </div>

          </div>
        )}

        {step === 'field' && (
          <div className={styles.downloads}>
            <div className={styles.download}>
              <span className={styles.downloadName}>speech</span>
              <div className={styles.track}>
                <div className={styles.fill} style={{ width: `${speechProgress.pct}%` }} />
              </div>
              <span className={styles.progressLabel}>{speechProgress.label}</span>
            </div>
            <div className={styles.download}>
              <span className={styles.downloadName}>question</span>
              <div className={styles.track}>
                <div className={styles.fill} style={{ width: `${reasoningProgress.pct}%` }} />
              </div>
              <span className={styles.progressLabel}>{reasoningProgress.label}</span>
            </div>
          </div>
        )}

        <div className={styles.go}>
          <span className={styles.note}>
            {step === 'models'
              ? 'Both start downloading now, so they run while you set the rest up.'
              : step === 'field'
                ? 'Speech lands first, so you can record as soon as it does. The question waits on the larger one.'
                : step === 'types'
                  ? 'It decides this itself. You can change it on any note.'
                  : 'Nothing here needs filing. It happens while you are not looking.'}{' '}
            Press <kbd className={styles.kbdInline}>Enter</kbd>.
          </span>
          <button
            type="button"
            className={styles.start}
            onClick={() => {
              if (step === 'models') return void chooseModels();
              if (step === 'field') return setStep('types');
              if (step === 'types') return setStep('links');
              void start();
            }}
          >
            {step === 'links' ? 'Start' : 'Continue'}
          </button>
        </div>
      </div>
    </div>
  );
}
