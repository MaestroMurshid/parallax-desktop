'use client';

import { useApp } from '@/lib/store';
import styles from './EmptyState.module.css';

/**
 * §14 — a few entries and no lines reads as broken. State the rule once, plainly,
 * and stop; the count is already in the status bar. Gone once any edge exists.
 */
export default function SparseNotice() {
  const count = useApp((s) => s.order.length);
  const hasEdges = useApp((s) => s.edges.some((e) => e.status !== 'dismissed'));

  // At one entry there is obviously nothing to pair, so the line is just noise.
  if (count < 2 || count > 6 || hasEdges) return null;

  return (
    <div className={styles.sparse}>
      <p className={styles.body}>
        Nothing proposed yet — it only suggests connections across time (§7.2).
        You can link any two entries yourself, whenever you like.
      </p>
    </div>
  );
}
