'use client';

import { legend, resolveTypes, type LegendRow, type RenderSlot } from '@/lib/scene/classification';
import { useApp } from '@/lib/store';
import MarkGlyph from './MarkGlyph';
import styles from './Legend.module.css';

/** Each label is set in its own letterform, so the legend is also a specimen. */
function letterform(slot: RenderSlot | null): React.CSSProperties {
  if (!slot) return {};
  return {
    fontFamily: slot.family === 'mono' ? 'var(--font-mono)' : 'var(--font-serif)',
    fontWeight: slot.weight,
    fontStyle: slot.italic ? 'italic' : 'normal',
    letterSpacing: `${slot.tracking}px`,
    opacity: slot.opacity,
  };
}

function Row({ row }: { row: LegendRow }) {
  return (
    <div className={styles.row}>
      <span className={styles.mark}>
        <MarkGlyph mark={row.mark} size={12} />
      </span>
      <span className={styles.label} style={letterform(row.slot)}>
        {row.label}
      </span>
      <span className={styles.gloss}>{row.gloss}</span>
    </div>
  );
}

export default function Legend() {
  const customTypes = useApp((s) => s.customTypes);
  const rows = legend(resolveTypes(customTypes));
  const built = rows.filter((r) => r.builtIn);
  const custom = rows.filter((r) => !r.builtIn);

  return (
    <aside className={styles.legend} aria-label="Legend">
      <span className={styles.heading}>legend</span>

      <div className={styles.rows}>
        {built.map((r) => (
          <Row key={r.label} row={r} />
        ))}
      </div>

      {custom.length > 0 && (
        <>
          <div className={styles.divider} />
          <span className={styles.custom}>yours ({customTypes.length})</span>
          <div className={styles.rows}>
            {custom.map((r) => (
              <Row key={r.label} row={r} />
            ))}
          </div>
        </>
      )}

      <div className={styles.divider} />
      <div className={styles.registerRow}>
        <span className={styles.registerSample}>live</span>
        <span className={styles.gloss}>
          something personal is at stake in it, so it is left alone unless you ask
        </span>
      </div>

      <div className={styles.question}>
        <span className={styles.dot} />
        <span className={styles.questionLabel}>open question</span>
      </div>
    </aside>
  );
}
