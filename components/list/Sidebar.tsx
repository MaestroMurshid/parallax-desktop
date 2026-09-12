'use client';

import { useMemo } from 'react';
import { BUILT_IN_TYPES, resolveTypes, slotFor } from '@/lib/scene/classification';
import { useApp } from '@/lib/store';
import type { RoleFilter } from './ListView';
import styles from './Sidebar.module.css';

/** Re-exported so a caller can type its own state off either half of the pair. */
export type { RoleFilter };

export interface SidebarProps {
  /** Controlled: the filter belongs to whoever composes the two, not to the store. */
  filter: RoleFilter;
  onFilterChange(next: RoleFilter): void;
}

/** `all` first, then the registry's own order — the legend already teaches
 *  position/evidence/note in that sequence, so the sidebar does not re-sort it. */
const FILTERS: Array<{ id: RoleFilter; label: string }> = [
  { id: 'all', label: 'All notes' },
  ...BUILT_IN_TYPES.map((t) => ({ id: t.id as RoleFilter, label: t.label })),
];

/**
 * Navigation, in the register of a notes app: no icons, no chevrons, nothing
 * that needs explaining on first contact. The canvas is one of the destinations
 * here rather than the thing you have to escape from.
 */
export default function Sidebar({ filter, onFilterChange }: SidebarProps) {
  const entries = useApp((s) => s.entries);
  const customTypes = useApp((s) => s.customTypes);
  const view = useApp((s) => s.view);
  const setView = useApp((s) => s.setView);

  const types = useMemo(() => resolveTypes(customTypes), [customTypes]);

  // Counts are the cheapest honest answer to "is this filter worth pressing".
  // Resolved the same way the list filters, or the two would disagree on a
  // note wearing a user-defined type's letterform.
  const counts = useMemo(() => {
    const n: Record<RoleFilter, number> = { all: 0, position: 0, evidence: 0, note: 0 };
    for (const entry of entries.values()) {
      n.all++;
      n[slotFor(entry, types)?.id ?? entry.role]++;
    }
    return n;
  }, [entries, types]);

  return (
    <nav className={styles.sidebar} aria-label="Notes">
      <ul className={styles.group}>
        {FILTERS.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              className={styles.item}
              aria-pressed={filter === item.id}
              onClick={() => onFilterChange(item.id)}
            >
              <span className={styles.label}>{item.label}</span>
              <span className={styles.count}>{counts[item.id]}</span>
            </button>
          </li>
        ))}
      </ul>

      {/* Bottom of the sidebar, not the top: the graph is the thing you go to
          once the list has already shown you there is something to connect. */}
      <div className={styles.views}>
        <span className={styles.heading}>View</span>
        <div className={styles.viewRow}>
          <button
            type="button"
            className={styles.view}
            aria-pressed={view === 'list'}
            onClick={() => setView('list')}
          >
            list
          </button>
          <button
            type="button"
            className={styles.view}
            aria-pressed={view === 'canvas'}
            onClick={() => setView('canvas')}
          >
            canvas
          </button>
        </div>
      </div>
    </nav>
  );
}
