'use client';

import { useEffect, useRef, useState, type ReactNode } from 'react';
import { getBridge } from '@/lib/bridge';
import { useApp } from '@/lib/store';
import type { ActionItem, Edge, Question, Span } from '@/lib/types';
import styles from './FileView.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });
const FENCE = '---';

/** What the frontmatter carries beyond the wire `Entry`. Every field is
 *  optional here: this panel reads a file, and a file is not a contract. */
interface Front {
  id?: string;
  title?: string;
  summary?: string | null;
  createdAt?: string;
  role?: string;
  register?: string;
  typeId?: string;
  durationMs?: number;
  x?: number;
  y?: number;
  isSample?: boolean;
  localOnly?: boolean;
  resolved?: boolean;
  unfinished?: boolean;
  resolutionText?: string | null;
  audioPath?: string | null;
  fingerprint?: number[];
  spans?: Span[];
  actionItems?: ActionItem[];
  edges?: Edge[];
  questions?: Question[];
  topics?: string[];
  anchors?: string[];
}

/** Split the way `mdx::parse` does: the first closing fence only, because a
 *  transcript is allowed to say `---` on a line of its own. */
function split(text: string): { front: Front; body: string } | null {
  const open = `${FENCE}\n`;
  if (!text.startsWith(open)) return null;
  const close = text.indexOf(`\n${FENCE}\n`, open.length - 1);
  if (close < 0) return null;
  try {
    const front = JSON.parse(text.slice(open.length, close)) as Front;
    const body = text
      .slice(close + FENCE.length + 2)
      .replace(/^\n/, '')
      .replace(/\n$/, '');
    return { front, body };
  } catch {
    return null;
  }
}

const TOKEN = /("(?:[^"\\]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b/g;

/** Tinted by weight rather than hue: the one colour in this design is spoken
 *  for, and the shape of the JSON is what needs to read. */
function tint(line: string): ReactNode[] {
  const out: ReactNode[] = [];
  let cursor = 0;
  for (const m of line.matchAll(TOKEN)) {
    const at = m.index ?? 0;
    if (at > cursor) out.push(line.slice(cursor, at));
    if (m[1] && m[2]) {
      out.push(
        <span key={at} className={styles.key}>
          {m[1]}
        </span>,
        m[2],
      );
    } else if (m[1]) {
      out.push(
        <span key={at} className={styles.string}>
          {m[1]}
        </span>,
      );
    } else {
      out.push(
        <span key={at} className={styles.literal}>
          {m[0]}
        </span>,
      );
    }
    cursor = at + m[0].length;
  }
  if (cursor < line.length) out.push(line.slice(cursor));
  return out;
}

function size(text: string): string {
  const bytes = new Blob([text]).size;
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} kB`;
}

function Source({ text }: { text: string }) {
  const lines = text.replace(/\n$/, '').split('\n');
  // The first fence opens and the next one closes; anything after is the body,
  // including a transcript's own `---`.
  let fences = 0;
  return (
    <div className={`${styles.source} selectable`} role="region" aria-label="File contents">
      {lines.map((line, i) => {
        const isFence = line === FENCE && fences < 2;
        if (isFence) fences += 1;
        const inBody = !isFence && fences >= 2;
        return (
          <div key={i} className={styles.line}>
            <span className={styles.gutter} aria-hidden>
              {i + 1}
            </span>
            <code className={isFence ? styles.fence : inBody ? styles.bodyLine : styles.code}>
              {isFence || inBody ? line || ' ' : tint(line)}
            </code>
          </div>
        );
      })}
    </div>
  );
}

function Fields({ front, body }: { front: Front; body: string }) {
  const entries = useApp((s) => s.entries);
  const openEntry = useApp((s) => s.openEntry);
  const self = front.id ?? '';
  const edges = front.edges ?? [];
  const questions = front.questions ?? [];
  const spans = (front.spans ?? []).filter((s) => s.attributed);
  const items = front.actionItems ?? [];
  const flags = [
    front.isSample && 'sample',
    front.localOnly && 'local only',
    front.resolved && 'resolved',
    front.unfinished && 'unfinished',
    front.audioPath === null && 'typed',
  ].filter(Boolean) as string[];
  // Offsets are UTF-16 code units and the body is the transcript, so slicing
  // the body is slicing what the offsets were frozen against.
  const quote = (span: Span) => body.slice(span.start, span.end);

  return (
    <div className={styles.fields}>
      <dl className={styles.facts}>
        {front.title && (
          <>
            <dt>title</dt>
            <dd className={styles.title}>{front.title}</dd>
          </>
        )}
        {front.createdAt && (
          <>
            <dt>recorded</dt>
            <dd>
              {dateFmt.format(new Date(front.createdAt))}
              {typeof front.durationMs === 'number' && ` · ${Math.round(front.durationMs / 1000)}s`}
            </dd>
          </>
        )}
        {front.role && (
          <>
            <dt>filed as</dt>
            <dd>{[front.role, front.register, front.typeId !== front.role && front.typeId].filter(Boolean).join(' · ')}</dd>
          </>
        )}
        {typeof front.x === 'number' && typeof front.y === 'number' && (
          <>
            <dt>placed at</dt>
            <dd className={styles.mono}>
              {Math.round(front.x)}, {Math.round(front.y)}
            </dd>
          </>
        )}
        {flags.length > 0 && (
          <>
            <dt>marked</dt>
            <dd>{flags.join(' · ')}</dd>
          </>
        )}
        {self && (
          <>
            <dt>id</dt>
            <dd className={styles.mono}>{self}</dd>
          </>
        )}
      </dl>

      {front.summary && <p className={styles.summary}>{front.summary}</p>}

      {((front.topics?.length ?? 0) > 0 || (front.anchors?.length ?? 0) > 0) && (
        <section className={styles.group}>
          <h3 className={styles.label}>filed under</h3>
          <div className={styles.chips}>
            {front.topics?.map((t) => (
              <span key={t} className={styles.chip}>
                {t}
              </span>
            ))}
          </div>
          {(front.anchors?.length ?? 0) > 0 && (
            <ul className={styles.anchors}>
              {front.anchors!.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
        </section>
      )}

      {edges.length > 0 && (
        <section className={styles.group}>
          <h3 className={styles.label}>
            {edges.length === 1 ? 'one connection' : `${edges.length} connections`}
          </h3>
          {edges.map((edge) => {
            const outgoing = edge.entryA === self;
            const otherId = outgoing ? edge.entryB : edge.entryA;
            const other = entries.get(otherId);
            return (
              <div key={edge.id} className={styles.item}>
                <div className={styles.itemHead}>
                  {/* The direction is part of the claim: `extends` from the
                      earlier note to the later one is not the same sentence
                      the other way round. */}
                  <span className={styles.relation}>
                    {outgoing ? `${edge.relation} →` : `← ${edge.relation}`}
                  </span>
                  {other ? (
                    <button type="button" className={styles.link} onClick={() => openEntry(other.id)}>
                      {other.title}
                    </button>
                  ) : (
                    <span className={styles.mono}>{otherId}</span>
                  )}
                  <span className={styles.status}>{edge.status}</span>
                </div>
                {edge.question && <p className={styles.question}>{edge.question}</p>}
              </div>
            );
          })}
        </section>
      )}

      {questions.length > 0 && (
        <section className={styles.group}>
          <h3 className={styles.label}>
            {questions.length === 1 ? 'one question' : `${questions.length} questions`}
          </h3>
          {questions.map((q) => (
            <div key={q.id} className={q.dismissed || q.answered ? styles.itemSettled : styles.item}>
              {q.span && <blockquote className={styles.quoted}>{quote(q.span)}</blockquote>}
              <p className={styles.question}>{q.text}</p>
              <p className={styles.provider}>
                {q.providerName}
                {q.answered && ' · answered'}
                {q.dismissed && ' · dismissed'}
              </p>
            </div>
          ))}
        </section>
      )}

      {spans.length > 0 && (
        <section className={styles.group}>
          <h3 className={styles.label}>someone else&rsquo;s words</h3>
          {spans.map((s) => (
            <blockquote key={`${s.start}-${s.end}`} className={styles.quoted}>
              {quote(s)}
            </blockquote>
          ))}
        </section>
      )}

      {items.length > 0 && (
        <section className={styles.group}>
          <h3 className={styles.label}>action items</h3>
          <ul className={styles.items}>
            {items.map((item) => (
              <li key={item.id} className={item.done ? styles.done : undefined}>
                {item.text}
              </li>
            ))}
          </ul>
        </section>
      )}

      {(front.fingerprint?.length ?? 0) > 0 && (
        <section className={styles.group}>
          <h3 className={styles.label}>fingerprint</h3>
          <div className={styles.fingerprint} aria-label="Amplitude over the recording">
            {front.fingerprint!.map((v, i) => (
              <span key={i} style={{ height: `${Math.max(6, Math.min(1, v) * 100)}%` }} />
            ))}
          </div>
        </section>
      )}

      <section className={styles.group}>
        <h3 className={styles.label}>body</h3>
        <p className={`${styles.body} selectable`}>{body}</p>
      </section>
    </div>
  );
}

/**
 * The note as the file it would be exported as, in place of the note itself.
 *
 * It replaces the entry sheet rather than opening inside it: the frontmatter
 * runs to a hundred lines, and squeezed under the transcript it was a box you
 * scrolled inside a panel you also scrolled. Read-only, because §5.1 freezes
 * offsets at insert -- the file records what was decided, not a form for it.
 */
export default function FileView() {
  const id = useApp((s) => s.selectedEntryId);
  const title = useApp((s) => (id ? s.entries.get(id)?.title : undefined));
  // Refetched whenever the corpus changes, so a connection landing while the
  // file is open shows up in it rather than leaving a stale copy on screen.
  const entries = useApp((s) => s.entries);
  const setOverlay = useApp((s) => s.setOverlay);
  const close = useApp((s) => s.closeOverlay);
  const [text, setText] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [tab, setTab] = useState<'file' | 'fields'>('file');
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const asked = useRef({ id, sent: 0, shown: 0 });

  // Measured in the packaged app: while enrichment runs, the corpus changes
  // every few seconds and each read waits seconds on the database, so dropping
  // a reply whenever the corpus moved left the panel on "Writing it out…"
  // until the model went quiet. A reply is kept unless a newer one already
  // landed or the panel has moved to another note.
  useEffect(() => {
    if (!id) return;
    if (asked.current.id !== id) {
      asked.current = { id, sent: 0, shown: 0 };
      setText(null);
      setFailed(false);
    }
    const n = ++asked.current.sent;
    const current = () => asked.current.id === id && n > asked.current.shown;
    getBridge()
      .entryMdx(id)
      .then((t) => {
        if (!current()) return;
        asked.current.shown = n;
        setText(t);
        setFailed(false);
      })
      .catch(() => {
        if (current()) setFailed(true);
      });
  }, [id, entries]);

  useEffect(
    () => () => {
      if (copyTimer.current) clearTimeout(copyTimer.current);
    },
    [],
  );

  if (!id) return null;
  const parsed = text === null ? null : split(text);

  return (
    <aside className={styles.sheet} aria-label={`${id}.mdx`}>
      <header className={styles.header}>
        <button type="button" className={styles.back} onClick={() => setOverlay('entry')}>
          ← {title ?? 'note'}
        </button>
        <div className={styles.headerActions}>
          {text !== null && (
            <button
              type="button"
              className={styles.headerAction}
              onClick={() => {
                void navigator.clipboard.writeText(text).then(() => {
                  setCopied(true);
                  if (copyTimer.current) clearTimeout(copyTimer.current);
                  copyTimer.current = setTimeout(() => setCopied(false), 1600);
                });
              }}
            >
              {copied ? 'copied' : 'copy'}
            </button>
          )}
          <button type="button" className={styles.headerAction} onClick={close} aria-label="Close">
            esc
          </button>
        </div>
      </header>

      <div className={styles.fileBar}>
        <span className={styles.fileName}>{id}.mdx</span>
        {text !== null && <span className={styles.fileSize}>{size(text)}</span>}
        <div className={styles.tabs} role="tablist">
          {(['file', 'fields'] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              className={tab === t ? styles.tabActive : styles.tab}
              // Fields needs a readable frontmatter; a file that does not parse
              // is still shown as the file.
              disabled={t === 'fields' && text !== null && parsed === null}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </div>
      </div>

      {failed && <p className={styles.empty}>This note could not be rendered as a file.</p>}
      {!failed && text === null && <p className={styles.empty}>Writing it out…</p>}
      {text !== null &&
        (tab === 'fields' && parsed ? <Fields front={parsed.front} body={parsed.body} /> : <Source text={text} />)}
    </aside>
  );
}
