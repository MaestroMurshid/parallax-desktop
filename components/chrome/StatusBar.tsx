'use client';

import { useEffect, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import type { ModelInfo, Settings } from '@/lib/types';
import styles from './StatusBar.module.css';

const mb = (bytes: number) => `${Math.round(bytes / 1e6)}MB`;

/** Everything about a model, for the hover. The strip stays short; this is
 *  where you look when a transcript came back worse than you expected. */
function detail(model: ModelInfo | undefined): string | undefined {
  if (!model) return undefined;
  return `${model.name} · ${model.params} params · ${model.quantization} · ${mb(model.sizeBytes)}`;
}

function stateLabel(model: ModelInfo | undefined): string {
  if (!model) return 'none';
  switch (model.state.kind) {
    case 'ready':
      return 'ready';
    case 'downloading':
      return `${Math.round((model.state.receivedBytes / model.state.totalBytes) * 100)}%`;
    case 'failed':
      return 'failed';
    case 'not-downloaded':
      return 'not downloaded';
  }
}

/**
 * Bottom chrome strip. Both models are shown because they mean different
 * things: speech gates recording, the question does not (§9.4).
 */
export default function StatusBar({ settings }: { settings: Settings }) {
  const entryCount = useApp((s) => s.order.length);
  const [models, setModels] = useState<ModelInfo[]>([]);

  useEffect(() => {
    const bridge = getBridge();
    void bridge.listModels().then(setModels);
    return bridge.onModelProgress((m) => {
      setModels((prev) => prev.map((x) => (x.id === m.id ? m : x)));
    });
  }, []);

  const speech = models.find(
    (m) => m.kind === 'transcription' && m.name === settings.transcriptionModel,
  );
  const reasoning = models.find((m) => m.id === settings.modelId);
  const speechReady = speech?.state.kind === 'ready';

  return (
    <footer className={styles.bar}>
      <span>
        {entryCount} {entryCount === 1 ? 'entry' : 'entries'}
      </span>

      <span className={styles.models}>
        {/* Both models are identified, not just stated ready. There are three
            transcription models and "base" alone names none of them, so it gets
            the same family-and-size treatment the reasoning model already had —
            which one ran is the first thing you want when a transcript comes
            back worse than you expected (§9.5). */}
        <span
          className={speechReady ? styles.model : styles.modelPending}
          title={detail(speech)}
        >
          {speech ? `whisper ${speech.name} ${speech.params}` : 'no speech model'}{' '}
          {stateLabel(speech)}
        </span>
        <span className={styles.divider} aria-hidden>
          ·
        </span>
        <span className={styles.model} title={detail(reasoning)}>
          {reasoning?.name ?? 'no model'} {stateLabel(reasoning)}
        </span>
      </span>
    </footer>
  );
}
