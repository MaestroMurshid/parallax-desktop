'use client';

import { useEffect, useRef, useState } from 'react';
import { APP_NAME } from '@/lib/constants';
import { useApp } from '@/lib/store';
import styles from './TopBar.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
const DEBOUNCE_MS = 120;
const MIN_QUERY = 2;

/** Splits a snippet around its match so the match can render in a <mark>. */
function markedSnippet(snippet: string, start: number, end: number) {
  return (
    <>
      {snippet.slice(0, start)}
      <mark className={styles.mark}>{snippet.slice(start, end)}</mark>
      {snippet.slice(end)}
    </>
  );
}

/**
 * Slim chrome strip: app name, search (results dim the canvas to matches),
 * and the tasks/settings overlay switches. Height and hairline read as
 * desktop chrome, not a web toolbar.
 */
export default function TopBar() {
  const [value, setValue] = useState('');
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const hits = useApp((s) => s.searchHits);
  const matchedIds = useApp((s) => s.matchedIds);
  const matchedEdgeIds = useApp((s) => s.matchedEdgeIds);
  const orphanIds = useApp((s) => s.orphanIds);
  const entries = useApp((s) => s.entries);
  const runSearch = useApp((s) => s.runSearch);
  const clearSearch = useApp((s) => s.clearSearch);
  const openEntry = useApp((s) => s.openEntry);
  const overlay = useApp((s) => s.overlay);
  const setOverlay = useApp((s) => s.setOverlay);
  const taskCount = useApp((s) => s.actionItems.filter((a) => !a.done).length);

  // Close the results panel on an outside click; Escape is handled separately.
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open]);

  useEffect(() => () => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
  }, []);

  const onChange = (v: string) => {
    setValue(v);
    setOpen(v.trim().length >= MIN_QUERY);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => void runSearch(v), DEBOUNCE_MS);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== 'Escape') return;
    e.stopPropagation(); // don't also trigger the app-wide Escape handling
    setValue('');
    setOpen(false);
    clearSearch();
    e.currentTarget.blur();
  };

  const pick = (entryId: string) => {
    openEntry(entryId);
    setOpen(false);
  };

  const showPanel = open && value.trim().length >= MIN_QUERY;
  const trimmed = value.trim();
  const quoted = trimmed.length > 2 && trimmed.startsWith('"') && trimmed.endsWith('"');

  return (
    <header className={styles.bar}>
      <span className={styles.appName}>{APP_NAME}</span>

      <div className={styles.searchWrap} ref={wrapRef}>
        <input
          type="text"
          className={styles.search}
          placeholder="search"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          onFocus={() => setOpen(value.trim().length >= MIN_QUERY)}
        />
        {showPanel && (
          <div className={styles.results}>
            {hits.length === 0 && <p className={styles.empty}>no matches</p>}
            {/* Discoverable where it is needed: you find out `ai` matched
                `maintain` by reading the snippets, and this is where they are. */}
            {hits.length > 0 && !quoted && (
              <p className={styles.hint}>
                <kbd className={styles.hintKey}>&ldquo;{trimmed}&rdquo;</kbd> for whole words only
              </p>
            )}
            {/* A result is a subgraph: say how much of it hangs together. */}
            {hits.length > 0 && matchedIds && (
              <p className={styles.summary}>
                {matchedIds.size} {matchedIds.size === 1 ? 'entry' : 'entries'}
                {matchedEdgeIds && matchedEdgeIds.size > 0 && (
                  <>
                    {' · '}
                    {matchedEdgeIds.size} {matchedEdgeIds.size === 1 ? 'link' : 'links'} between them
                  </>
                )}
                {orphanIds && orphanIds.size > 0 && (
                  <>
                    {' · '}
                    <span className={styles.orphan}>{orphanIds.size} standing alone</span>
                  </>
                )}
              </p>
            )}
            {hits.map((hit) => {
              const entry = entries.get(hit.entryId);
              if (!entry) return null;
              return (
                <button
                  key={`${hit.entryId}-${hit.start}`}
                  type="button"
                  className={styles.result}
                  onMouseDown={() => pick(hit.entryId)}
                >
                  <span className={styles.resultHead}>
                    <span className={styles.resultTitle}>{entry.title}</span>
                    <span className={styles.resultDate}>{dateFmt.format(new Date(entry.createdAt))}</span>
                  </span>
                  <span className={styles.snippet}>
                    {markedSnippet(hit.snippet, hit.snippetStart, hit.snippetEnd)}
                  </span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      <div className={styles.right}>
        <button
          type="button"
          className={styles.textButton}
          onClick={() => setOverlay(overlay === 'tasks' ? 'none' : 'tasks')}
        >
          tasks{taskCount > 0 ? ` · ${taskCount}` : ''}
        </button>
        <button
          type="button"
          className={styles.textButton}
          onClick={() => setOverlay(overlay === 'settings' ? 'none' : 'settings')}
        >
          settings
        </button>
      </div>
    </header>
  );
}
