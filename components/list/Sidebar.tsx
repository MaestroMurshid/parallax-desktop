'use client';

import { useMemo } from 'react';
import MarkGlyph from '@/components/canvas/MarkGlyph';
import { BUILT_IN_TYPES, resolveTypes, slotFor, type Mark, type TypeDefinition } from '@/lib/scene/classification';
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

  const types = useMemo(() => resolveTypes(customTypes), [customTypes]);

  // The same test the canvas legend uses: a type with nothing to draw (no
  // letterform, no mark of its own) has no distinct face to file a note
  // under, so it earns no row here either.
  const customRows = useMemo(
    () => types.filter((t) => !t.builtIn && (t.role || t.mark)),
    [types],
  );

  // Counts are the cheapest honest answer to "is this filter worth pressing".
  // Built-ins resolve the same way the list filters, or the two would
  // disagree on a note wearing a user-defined type's letterform; a custom
  // type counts by its own exact id, since two of them can share a letterform
  // and still need separate counts (`matchesFilter` in ListView mirrors this).
  const counts = useMemo(() => {
    const n: Record<RoleFilter, number> = { all: 0, position: 0, evidence: 0, note: 0 };
    for (const row of customRows) n[row.id] = 0;
    for (const entry of entries.values()) {
      n.all = (n.all ?? 0) + 1;
      const role = slotFor(entry, types)?.id ?? entry.role;
      n[role] = (n[role] ?? 0) + 1;
      if (entry.typeId in n) n[entry.typeId] = (n[entry.typeId] ?? 0) + 1;
    }
    return n;
  }, [entries, types, customRows]);

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

      {customRows.length > 0 && (
        <ul className={styles.group}>
          {customRows.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                className={styles.item}
                aria-pressed={filter === t.id}
                onClick={() => onFilterChange(t.id)}
              >
                <MarkGlyph mark={markOf(t.mark, t.role)} size={11} />
                <span className={styles.label}>{t.label}</span>
                <span className={styles.count}>{counts[t.id] ?? 0}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </nav>
  );
}

/** The legend's own rule for which mark to draw: the type's own mark if it
 *  set one, otherwise its letterform's glyph. */
function markOf(mark: Mark | null, role: TypeDefinition['role']): Mark | null {
  return mark ?? (role ? { kind: 'glyph', id: role } : null);
}
