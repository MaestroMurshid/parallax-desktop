'use client';

import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { getBridge, type SearchHit } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import type { Entry } from '@/lib/types';
import styles from './ChatPanel.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });

/**
 * The frame a spoken question is wrapped in. None of it is ever the subject,
 * and all of it is common enough to match half the corpus as a substring.
 * Deliberately a fixed list rather than a frequency cut over the corpus: a
 * stoplist computed from your own notes starts dropping the words you talk
 * about most, which are exactly the ones you came here to find.
 */
const FRAME_WORDS = new Set([
  'a', 'about', 'again', 'ago', 'all', 'an', 'and', 'any', 'anything', 'are', 'around', 'as',
  'at', 'back', 'be', 'been', 'but', 'by', 'can', 'could', 'did', 'do', 'does', 'ever', 'first',
  'for', 'from', 'get', 'got', 'had', 'has', 'have', 'how', 'i', 'if', 'in', 'into', 'is', 'it',
  'me', 'mine', 'my', 'myself', 'note', 'notes', 'of', 'on', 'once', 'or', 'out', 'over', 'said',
  'say', 'saying', 'should', 'so', 'some', 'something', 'that', 'the', 'their', 'then', 'there',
  'these', 'they', 'think', 'thinking', 'those', 'thought', 'to', 'told', 'up', 'was', 'were',
  'what', 'whats', 'when', 'where', 'which', 'who', 'whom', 'why', 'will', 'with', 'would',
]);

/** Enough subject words for a real question; a cap because each one is a call. */
const MAX_TERMS = 6;
/** At or under this length a substring match is meaningless — `ai` finds maintain. */
const SHORT_TERM = 3;

type Status = 'idle' | 'searching' | 'answered' | 'failed';

/** What one question put to the corpus and what came back, kept together so a
 *  half-edited input never relabels the results already on screen. */
interface Answer {
  question: string;
  /** The words actually searched, in display form — the user is owed this when
   *  the reply is "nothing matched". */
  searched: string[];
  hits: Array<{ hit: SearchHit; term: string }>;
}

interface Recalled {
  entry: Entry;
  passages: SearchHit[];
  matched: string[];
}

const bare = (term: string) => (term.startsWith('"') ? term.slice(1, -1) : term);

/**
 * A question is not a search string. `searchEntries` matches substrings, so
 * "what did I say about deadlines" put through whole matches nothing at all —
 * the panel would look broken rather than empty. Stripping the frame leaves the
 * words that could plausibly be in a transcript, and each is searched alone.
 */
function termsFor(question: string): string[] {
  const raw = question.trim();
  // Already exact: a quoted query is the bridge's whole-word mode, and it should
  // be reachable from here for the same reason it is from the top bar.
  if (raw.length > 2 && raw.startsWith('"') && raw.endsWith('"')) return [raw];

  const words = raw.toLowerCase().split(/[^\p{L}\p{N}'-]+/u).filter(Boolean);
  const subject = words.filter((w) => w.length > 1 && !FRAME_WORDS.has(w));
  // All frame and no subject ("what did I say?") — search it as typed. Searching
  // nothing and reporting no matches would be a lie about what was looked at.
  const chosen = (subject.length > 0 ? subject : words).slice(0, MAX_TERMS);
  return chosen.map((w) => (w.length <= SHORT_TERM ? `"${w}"` : w));
}

/**
 * The match, re-based into the snippet by the bridge. Offsets are UTF-16 code
 * units and `String.prototype.slice` indexes the same units, so the two agree
 * with no conversion — it already happened where the offset was produced
 * (src-tauri/src/text.rs). Anything that walks code points instead (Array.from,
 * spread, Intl.Segmenter) reopens the bug that landed highlights two characters
 * out on every note containing an em dash.
 */
function marked(snippet: string, start: number, end: number) {
  return (
    <>
      {snippet.slice(0, start)}
      <mark className={styles.mark}>{snippet.slice(start, end)}</mark>
      {snippet.slice(end)}
    </>
  );
}

/**
 * Recall against dated transcripts. It finds and shows; it does not answer.
 * There is no model call anywhere in here on purpose: the reply to a question
 * is the user's own sentences with their dates, never a sentence about them,
 * and a note that does not exist has no paraphrase that could stand in for it.
 *
 * Self-gating on `chatOpen` rather than prop-driven, like ConnectPicker — and
 * it keeps its last answer while closed, so reopening resumes where you were.
 */
export default function ChatPanel() {
  const chatOpen = useApp((s) => s.chatOpen);
  const setChatOpen = useApp((s) => s.setChatOpen);
  const entries = useApp((s) => s.entries);
  const order = useApp((s) => s.order);
  const loaded = useApp((s) => s.loaded);
  const openEntry = useApp((s) => s.openEntry);

  const [value, setValue] = useState('');
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [status, setStatus] = useState<Status>('idle');

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const runRef = useRef(0);

  // Focus goes in on open and back where it came from on close. The panel does
  // not trap it: the canvas behind stays live and Tab should be able to leave.
  useEffect(() => {
    if (!chatOpen) return;
    returnFocusRef.current = document.activeElement as HTMLElement | null;
    inputRef.current?.focus();
    inputRef.current?.select();
    return () => returnFocusRef.current?.focus();
  }, [chatOpen]);

  const ask = useCallback(async (question: string) => {
    const asked = question.trim();
    if (!asked) return;
    const terms = termsFor(asked);
    const run = ++runRef.current;
    setStatus('searching');
    try {
      const bridge = getBridge();
      const found = await Promise.all(
        terms.map(async (term) => ({ term: bare(term), hits: await bridge.searchEntries(term) })),
      );
      if (runRef.current !== run) return; // a later question already went out
      setAnswer({
        question: asked,
        searched: terms.map(bare),
        hits: found.flatMap(({ term, hits }) => hits.map((hit) => ({ hit, term }))),
      });
      setStatus('answered');
    } catch {
      if (runRef.current !== run) return;
      setStatus('failed');
    }
  }, []);

  const recalled = useMemo<Recalled[]>(() => {
    if (!answer) return [];
    const byEntry = new Map<string, { passages: Map<string, SearchHit>; matched: Set<string> }>();
    for (const { hit, term } of answer.hits) {
      let group = byEntry.get(hit.entryId);
      if (!group) {
        group = { passages: new Map(), matched: new Set() };
        byEntry.set(hit.entryId, group);
      }
      // Two terms can land on one passage; the note shows it once either way.
      group.passages.set(`${hit.start}:${hit.end}`, hit);
      group.matched.add(term);
    }

    const out: Recalled[] = [];
    // `order` is insertion order, which is chronological and is also what §5.1
    // places by. Walked backwards rather than sorted on createdAt so "most
    // recent" means the same thing here, in the list and on the canvas — sorting
    // disagrees with all three as soon as two notes share a timestamp.
    for (let i = order.length - 1; i >= 0; i--) {
      const id = order[i];
      const group = id ? byEntry.get(id) : undefined;
      const entry = id ? entries.get(id) : undefined;
      if (!group || !entry) continue;
      out.push({
        entry,
        passages: [...group.passages.values()].sort((a, b) => a.start - b.start),
        matched: [...group.matched],
      });
    }
    return out;
  }, [answer, order, entries]);

  // Tab already reaches every row — they are real buttons. Arrows are what a
  // long result set needs so twenty notes is not twenty Tabs.
  function onListKeyDown(e: KeyboardEvent<HTMLUListElement>) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const host = listRef.current;
    if (!host) return;
    const rows = [...host.querySelectorAll<HTMLButtonElement>('[data-row]')];
    const at = rows.indexOf(document.activeElement as HTMLButtonElement);
    if (at === -1) return;
    e.preventDefault();
    const next =
      e.key === 'Home' ? 0
        : e.key === 'End' ? rows.length - 1
          : at + (e.key === 'ArrowDown' ? 1 : -1);
    rows[Math.max(0, Math.min(rows.length - 1, next))]?.focus();
  }

  function onPanelKeyDown(e: KeyboardEvent<HTMLElement>) {
    if (e.key !== 'Escape') return;
    // Stopped here rather than left to the page handler, which would close the
    // entry sheet behind this panel on the same press. One press, one dismissal.
    e.stopPropagation();
    setChatOpen(false);
  }

  if (!chatOpen) return null;

  const passageCount = recalled.reduce((n, r) => n + r.passages.length, 0);
  const oldest = recalled[recalled.length - 1]?.entry;
  const edited = answer !== null && value.trim() !== answer.question;

  return (
    <aside className={styles.sheet} aria-label="Recall" onKeyDown={onPanelKeyDown}>
      <header className={styles.header}>
        <span className={styles.meta}>recall</span>
        <button
          type="button"
          className={styles.close}
          aria-label="Close recall"
          onClick={() => setChatOpen(false)}
        >
          esc
        </button>
      </header>

      <form
        className={styles.form}
        onSubmit={(e) => {
          e.preventDefault();
          void ask(value);
        }}
      >
        <label className={styles.label} htmlFor="recall-question">
          ask your own notes
        </label>
        <input
          id="recall-question"
          ref={inputRef}
          type="text"
          className={styles.field}
          value={value}
          placeholder="what did I say about deadlines"
          disabled={!loaded}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            // Straight from the question into the results, without a Tab past
            // the status line to get there.
            if (e.key === 'ArrowDown' && recalled.length > 0) {
              e.preventDefault();
              listRef.current?.querySelector<HTMLButtonElement>('[data-row]')?.focus();
            }
          }}
        />
      </form>

      <p className={styles.status} role="status">
        {status === 'searching' && 'searching your notes…'}
        {status === 'failed' && 'the search did not run — ask again'}
        {status === 'answered' && recalled.length > 0 && (
          <>
            {recalled.length} {recalled.length === 1 ? 'note' : 'notes'}
            {' · '}
            {passageCount} {passageCount === 1 ? 'passage' : 'passages'}
            {/* A question is as often "when did I first say this" as "what did I
                say": the earliest date answers that without reordering. */}
            {oldest && <> · earliest {dateFmt.format(new Date(oldest.createdAt))}</>}
          </>
        )}
        {status === 'answered' && recalled.length === 0 && 'no matches'}
      </p>

      {answer && status !== 'searching' && (
        <p className={styles.searched}>
          searched{' '}
          {answer.searched.map((t) => (
            <span key={t} className={styles.term}>
              {t}
            </span>
          ))}
        </p>
      )}

      {!answer && status !== 'searching' && (
        <p className={styles.empty}>
          Ask what you said about something. This searches the transcripts and shows you the
          notes themselves — your words, dated, with a way back to each one. It never answers
          for you and it never adds anything to the corpus.
        </p>
      )}

      {status === 'answered' && recalled.length === 0 && (
        <p className={styles.empty}>
          No note contains those words. That is the whole answer — nothing here is guessed at,
          summarised or written for you, so an empty result means you have not said it yet.
          Try a different word, or quote a phrase to match it whole.
        </p>
      )}

      <ul className={styles.list} ref={listRef} onKeyDown={onListKeyDown}>
        {recalled.map(({ entry, passages, matched }) => (
          <li key={entry.id}>
            {/* The whole result is one control: the passage is the way back to
                the note, so making it inert text and the title a separate button
                would put the click somewhere other than where the eye is. */}
            <button
              type="button"
              data-row
              className={styles.row}
              onClick={() => openEntry(entry.id)}
            >
              <span className={styles.rowHead}>
                <span className={styles.rowTitle}>{entry.title}</span>
                <span className={styles.rowDate}>
                  {dateFmt.format(new Date(entry.createdAt))}
                </span>
              </span>
              {passages.map((hit) => (
                <span key={`${hit.start}:${hit.end}`} className={styles.passage}>
                  {marked(hit.snippet, hit.snippetStart, hit.snippetEnd)}
                </span>
              ))}
              {answer && answer.searched.length > 1 && (
                <span className={styles.matched}>matched {matched.join(' · ')}</span>
              )}
            </button>
          </li>
        ))}
      </ul>

      <p className={styles.note}>
        {edited ? (
          <>
            <kbd className={styles.kbd}>enter</kbd> to ask this one
          </>
        ) : (
          <>
            <kbd className={styles.kbd}>↑</kbd> <kbd className={styles.kbd}>↓</kbd> to move,{' '}
            <kbd className={styles.kbd}>enter</kbd> to open the note,{' '}
            <kbd className={styles.kbd}>esc</kbd> to close.
          </>
        )}
      </p>
    </aside>
  );
}
