'use client';

import { useEffect, useRef } from 'react';
import { getBridge } from '@/lib/bridge';
import styles from './CapturePanel.module.css';

const BARS = 9;

/**
 * The only animation in the app (§8). Amplitude is written straight to DOM
 * refs — routing 30 samples a second through the store would re-render the
 * panel to move nine bars.
 */
export default function Equalizer({ frozen }: { frozen?: number[] }) {
  const barsRef = useRef<Array<HTMLSpanElement | null>>([]);
  const levels = useRef<number[]>(Array(BARS).fill(0.08));

  useEffect(() => {
    if (frozen) {
      frozen.slice(0, BARS).forEach((level, i) => {
        const el = barsRef.current[i];
        if (el) el.style.height = `${Math.max(8, level * 100)}%`;
      });
      return;
    }

    return getBridge().onAmplitude((level) => {
      levels.current = [level, ...levels.current.slice(0, BARS - 1)];
      for (let i = 0; i < BARS; i++) {
        const el = barsRef.current[i];
        if (el) el.style.height = `${Math.max(8, (levels.current[i] ?? 0) * 100)}%`;
      }
    });
  }, [frozen]);

  return (
    <div className={styles.equalizer} aria-hidden>
      {Array.from({ length: BARS }, (_, i) => (
        <span
          key={i}
          ref={(el) => {
            barsRef.current[i] = el;
          }}
          className={styles.bar}
        />
      ))}
    </div>
  );
}
