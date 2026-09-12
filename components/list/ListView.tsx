'use client';

import { useMemo, useRef, type CSSProperties, type KeyboardEvent } from 'react';
import {
  BUILT_IN_TYPES,
  resolveTypes,
  slotFor,
  typeLabel,
  type TypeDefinition,
} from '@/lib/scene/classification';
import { useApp } from '@/lib/store';
import type { Entry, Role } from '@/lib/types';
import styles from './ListView.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });

/** Long enough to recognise a note you recorded, short enough that the list
 *  still scans. The record itself is one click away and never abridged (§1.1). */
const EXCERPT_CHARS = 190;

/**
 * `all` is a member of the union rather than `null` so the sidebar's four
 * controls are one exhaustive switch, and an unfiltered list needs no sentinel.
 */
export type RoleFilter = Role | 'all';

export interface ListViewProps {
  /** Defaults to everything, so the list is useful before anything is wired to it. */
  filter?: RoleFilter;
}

/** Whitespace is collapsed because a transcript carries the pauses of speech as
 *  line breaks, and those turn a three-line excerpt into a ragged column. */
function excerpt(transcript: string): string {
  const flat = transcript.replace(/\s+/g, ' ').trim();
  if (flat.length <= EXCERPT_CHARS) return flat;
  const cut = flat.slice(0, EXCERPT_CHARS);
  const space = cut.lastIndexOf(' ');
  return `${(space > EXCERPT_CHARS * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

/**
 * Same move EntryView makes for a linked title: the row wears the letterform its
 * role gets on the canvas, so the two surfaces speak one vocabulary. Opacity is
 * deliberately left behind — the canvas can dim a note to 0.7 against a blob,
 * a body-copy row cannot without losing contrast.
 */
function letterform(entry: Entry, types: TypeDefinition[]): CSSProperties {
  const slot = slotFor(entry, types);
  if (!slot) return {};
  return {
    fontFamily: slot.family === 'mono' ? 'var(--font-mono)' : 'var(--font-serif)',
    fontWeight: slot.weight,
    letterSpacing: `${slot.tracking}px`,
  };
}

/**
 * The default way in. Newest first, because a list you read top-down is how
 * anyone already reads a commonplace book — the canvas answers "what connects
 * to what", which is a second question, not the first one.
 */
export default function ListView({ filter = 'all' }: ListViewProps) {
  const order = useApp((s) => s.order);
  const entries = useApp((s) => s.entries);
  const questions = useApp((s) => s.questions);
  const customTypes = useApp((s) => s.customTypes);
  const loaded = useApp((s) => s.loaded);
  const openEntry = useApp((s) => s.openEntry);
  const listRef = useRef<HTMLUListElement>(null);

  const types = useMemo(() => resolveTypes(customTypes), [customTypes]);

  const rows = useMemo(() => {
    const out: Entry[] = [];
    // `order` is insertion order, which §5.1 also uses for placement — reversing
    // here rather than sorting keeps the list agreeing with the canvas about
    // what "later" means, including for entries that share a timestamp.
    for (let i = order.length - 1; i >= 0; i--) {
      const id = order[i];
      const entry = id ? entries.get(id) : undefined;
      if (!entry) continue;
      // The resolved role, not `entry.role`: a user-defined type binds its own
      // letterform (§3.6), and filing a note under the face it wears is the
      // only answer that matches what is on screen.
      if (filter !== 'all' && (slotFor(entry, types)?.id ?? entry.role) !== filter) continue;
      out.push(entry);
    }
    return out;
  }, [order, entries, types, filter]);

  // Tab reaches every row on its own — these are real buttons. Arrows are the
  // addition a long list needs, so moving through fifty notes is not fifty Tabs.
  function onKeyDown(e: KeyboardEvent<HTMLUListElement>) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const host = listRef.current;
    if (!host) return;
    const cells = [...host.querySelectorAll<HTMLButtonElement>('[data-row]')];
    const at = cells.indexOf(document.activeElement as HTMLButtonElement);
    if (at === -1) return;
    e.preventDefault();
    const next =
      e.key === 'Home' ? 0
        : e.key === 'End' ? cells.length - 1
          : at + (e.key === 'ArrowDown' ? 1 : -1);
    cells[Math.max(0, Math.min(cells.length - 1, next))]?.focus();
  }

  if (!loaded) return <div className={styles.view} />;

  if (rows.length === 0) {
    const gloss = BUILT_IN_TYPES.find((t) => t.id === filter)?.match;
    return (
      <div className={styles.view}>
        <div className={styles.empty}>
          {gloss ? (
            <p className={styles.emptyBody}>
              Nothing filed as <span className={styles.emptyTerm}>{filter}</span> yet — {gloss}.
              Every note is read back and filed after you record it.
            </p>
          ) : (
            <p className={styles.emptyBody}>
              Every note you record lands here, newest first: the transcript as you said
              it, under a title taken from your own words. Nothing has been recorded yet.
            </p>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={styles.view}>
      <ul className={styles.list} ref={listRef} onKeyDown={onKeyDown}>
        {rows.map((entry) => {
          const open = (questions.get(entry.id) ?? []).some((q) => !q.answered && !q.dismissed);
          return (
            <li key={entry.id}>
              <button
                type="button"
                data-row
                className={styles.row}
                onClick={() => openEntry(entry.id)}
              >
                <span className={styles.head}>
                  <span className={styles.title} style={letterform(entry, types)}>
                    {entry.title}
                  </span>
                  {/* The one saturated colour in the app, spent where §8 says it
                      may be spent. Without it the list would be the only surface
                      that cannot tell you which notes are still open. */}
                  {open && <span className={styles.open} role="img" aria-label="open question" />}
                  <span className={styles.role}>{typeLabel(entry, types)}</span>
                </span>
                <span className={styles.meta}>
                  {dateFmt.format(new Date(entry.createdAt))}
                  {' · '}
                  {Math.round(entry.durationMs / 1000)}s
                  {entry.audioPath === null && ' · typed'}
                  {entry.localOnly && ' · local only'}
                </span>
                <span className={styles.excerpt}>{excerpt(entry.transcript)}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
