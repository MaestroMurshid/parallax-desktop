'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { useApp } from '@/lib/store';
import styles from './ConnectPicker.module.css';

/**
 * The pointer-free half of manual linking (§5.4). The drag handle needs hover
 * and aim; this needs neither, so touch and keyboard reach the same picker.
 */
export default function ConnectPicker() {
  const fromId = useApp((s) => s.connectSource);
  const setConnectSource = useApp((s) => s.setConnectSource);
  const setPendingLink = useApp((s) => s.setPendingLink);
  const entries = useApp((s) => s.entries);
  const order = useApp((s) => s.order);
  const edges = useApp((s) => s.edges);

  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(0);
  const fieldRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const linked = useMemo(() => {
    const set = new Set<string>();
    for (const e of edges) {
      if (e.entryA === fromId) set.add(e.entryB);
      if (e.entryB === fromId) set.add(e.entryA);
    }
    return set;
  }, [edges, fromId]);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    return order
      .map((id) => entries.get(id))
      .filter((e) => !!e && e.id !== fromId && e.parentEdge === null && !linked.has(e.id))
      .filter((e) => !q || e!.title.toLowerCase().includes(q) || e!.transcript.toLowerCase().includes(q))
      .slice(0, 40) as NonNullable<ReturnType<typeof entries.get>>[];
  }, [order, entries, fromId, linked, query]);

  useEffect(() => {
    if (fromId) {
      setQuery('');
      setCursor(0);
      fieldRef.current?.focus();
    }
  }, [fromId]);

  useEffect(() => {
    listRef.current?.querySelector('[data-at]')?.scrollIntoView({ block: 'nearest' });
  }, [cursor]);

  if (!fromId) return null;
  const from = entries.get(fromId);
  if (!from) return null;

  const choose = (toId: string) => setPendingLink({ fromId, toId });

  const onKeyDown = (e: React.KeyboardEvent) => {
    // Captured here rather than on window so the page hotkeys stay untouched.
    if (e.key === 'Escape') {
      e.stopPropagation();
      setConnectSource(null);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setCursor((c) => Math.min(c + 1, matches.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    } else if (e.key === 'Enter' && matches[cursor]) {
      e.preventDefault();
      choose(matches[cursor].id);
    }
  };

  return (
    <div className={styles.scrim} onMouseDown={() => setConnectSource(null)}>
      <div
        className={styles.picker}
        role="dialog"
        aria-label="Connect this entry to another"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <p className={styles.pair}>
          <span className={styles.node}>{from.title}</span>
          <span className={styles.blank}>connects to…</span>
        </p>

        <input
          ref={fieldRef}
          className={styles.field}
          value={query}
          placeholder="filter by title or transcript"
          aria-label="Filter entries"
          onChange={(e) => {
            setQuery(e.target.value);
            setCursor(0);
          }}
        />

        <div className={styles.list} ref={listRef} role="listbox" aria-label="Entries">
          {matches.map((e, i) => (
            <button
              key={e.id}
              type="button"
              role="option"
              aria-selected={i === cursor}
              data-at={i === cursor ? '' : undefined}
              className={i === cursor ? styles.rowOn : styles.row}
              onMouseEnter={() => setCursor(i)}
              onClick={() => choose(e.id)}
            >
              <span className={styles.title}>{e.title}</span>
              <span className={styles.when}>{new Date(e.createdAt).toLocaleDateString()}</span>
            </button>
          ))}
          {matches.length === 0 && (
            <p className={styles.nothing}>
              {query ? 'No entry matches that.' : 'Everything else is already connected to this one.'}
            </p>
          )}
        </div>

        <p className={styles.note}>
          <kbd className={styles.kbd}>↑</kbd> <kbd className={styles.kbd}>↓</kbd> to move,{' '}
          <kbd className={styles.kbd}>enter</kbd> to pick the relation,{' '}
          <kbd className={styles.kbd}>esc</kbd> to leave it.
        </p>
      </div>
    </div>
  );
}
