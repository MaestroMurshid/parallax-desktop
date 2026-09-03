'use client';

import { useEffect } from 'react';
import { useApp } from '@/lib/store';
import type { Relation } from '@/lib/types';
import styles from './RelationPicker.module.css';

/** §5.4 — the whole vocabulary. "Related" is deliberately not in it. */
const RELATIONS: Array<{ id: Relation; gloss: string }> = [
  { id: 'contradicts', gloss: 'the later one takes it back' },
  { id: 'same move', gloss: 'different subject, same argument' },
  { id: 'returns to', gloss: 'picks the earlier one back up' },
  { id: 'questions', gloss: 'asks something of it' },
  { id: 'extends', gloss: 'carries it further' },
  { id: 'example of', gloss: 'an instance of the other' },
  { id: 'echoes', gloss: 'rhymes without arguing' },
];

/**
 * Naming the relation is the step the research says carries the benefit, so it
 * is a required choice rather than a default (§5.4).
 */
export default function RelationPicker() {
  const link = useApp((s) => s.pendingLink);
  const setPendingLink = useApp((s) => s.setPendingLink);
  const linkEntries = useApp((s) => s.linkEntries);
  const entries = useApp((s) => s.entries);

  useEffect(() => {
    if (!link) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setPendingLink(null);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [link, setPendingLink]);

  if (!link) return null;
  const from = entries.get(link.fromId);
  const to = entries.get(link.toId);
  if (!from || !to) return null;

  return (
    <div className={styles.scrim} onMouseDown={() => setPendingLink(null)}>
      <div className={styles.picker} onMouseDown={(e) => e.stopPropagation()}>
        <p className={styles.pair}>
          <span className={styles.node}>{from.title}</span>
          <span className={styles.blank}>…</span>
          <span className={styles.node}>{to.title}</span>
        </p>

        <div className={styles.options}>
          {RELATIONS.map((r) => (
            <button
              key={r.id}
              type="button"
              className={styles.relation}
              onClick={() => {
                void linkEntries(link.fromId, link.toId, r.id);
                setPendingLink(null);
              }}
            >
              <span className={styles.word}>{r.id}</span>
              <span className={styles.gloss}>{r.gloss}</span>
            </button>
          ))}
        </div>

        <p className={styles.note}>
          If none of these fit, the connection probably isn&rsquo;t one —{' '}
          <button type="button" className={styles.cancel} onClick={() => setPendingLink(null)}>
            leave it
          </button>
          .
        </p>
      </div>
    </div>
  );
}
