'use client';

import { useApp } from '@/lib/store';
import styles from './TaskList.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short' });

/**
 * §1.2 — one lightweight global list. Ticking is state on the span, so the
 * transcript is never edited; the source entry is one click away.
 */
export default function TaskList() {
  const actionItems = useApp((s) => s.actionItems);
  const entries = useApp((s) => s.entries);
  const toggle = useApp((s) => s.toggleActionItem);
  const openEntry = useApp((s) => s.openEntry);
  const close = useApp((s) => s.closeOverlay);

  return (
    <aside className={styles.sheet}>
      <header className={styles.header}>
        <span className={styles.meta}>{actionItems.length} items</span>
        <button type="button" className={styles.close} onClick={close} aria-label="Close">
          esc
        </button>
      </header>

      {actionItems.length === 0 && (
        <p className={styles.empty}>
          Nothing yet. Action items are picked out of what you say and collected here.
        </p>
      )}

      <ul className={styles.list}>
        {actionItems.map((item) => {
          const entry = entries.get(item.entryId);
          return (
            <li key={item.id} className={styles.row}>
              <label className={styles.task}>
                <input
                  type="checkbox"
                  checked={item.done}
                  onChange={() => void toggle(item.id)}
                />
                <span className={item.done ? styles.done : undefined}>{item.text}</span>
              </label>
              {entry && (
                <button
                  type="button"
                  className={styles.source}
                  onClick={() => openEntry(entry.id)}
                >
                  {entry.title} · {dateFmt.format(new Date(entry.createdAt))}
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </aside>
  );
}
