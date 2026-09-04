'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { hasOwnSpan, invokedProbes, resolveTypes, typeLabel } from '@/lib/scene/classification';
import { useApp } from '@/lib/store';
import type { Edge, Entry, Question, Span } from '@/lib/types';
import styles from './EntryView.module.css';

const dateFmt = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric' });

function segments(transcript: string, spans: Span[]) {
  const attributed = spans.filter((s) => s.attributed).sort((a, b) => a.start - b.start);
  const out: Array<{ text: string; attributed: boolean }> = [];
  let cursor = 0;
  for (const span of attributed) {
    if (span.start > cursor) out.push({ text: transcript.slice(cursor, span.start), attributed: false });
    out.push({ text: transcript.slice(span.start, span.end), attributed: true });
    cursor = span.end;
  }
  if (cursor < transcript.length) out.push({ text: transcript.slice(cursor), attributed: false });
  return out;
}

const EMPTY: Question[] = [];

/**
 * §5.3's best decision is currently invisible: a felt entry and a broken one
 * say the same eleven words. Name the reason, and the guardrail becomes the
 * feature rather than reading as a failure.
 */
function silenceReason(entry: Entry): string {
  if (entry.role === 'note') return 'A note — kept as written.';
  if (entry.role === 'evidence') return 'It won’t argue with something you noticed. Select a sentence and it will ask you to say it back.';
  if (!hasOwnSpan(entry)) return "Every word here is someone else's. Nothing of yours to push on.";
  if (entry.register === 'live') return 'Left alone — this one reads as live. Select a sentence to push it anyway.';
  if (entry.durationMs < 30_000) return 'Under thirty seconds — said once, not interrogated.';
  return 'Nothing proposed for this entry.';
}

export default function EntryView({ hotkey }: { hotkey: string }) {
  const id = useApp((s) => s.selectedEntryId);
  const entry = useApp((s) => (id ? s.entries.get(id) : undefined));
  const questions = useApp((s) => (id ? s.questions.get(id) : undefined)) ?? EMPTY;
  const analysisOpen = useApp((s) => s.analysisOpen);
  const toggleAnalysis = useApp((s) => s.toggleAnalysis);
  const setConnectSource = useApp((s) => s.setConnectSource);
  const close = useApp((s) => s.closeOverlay);
  const dismissEdge = useApp((s) => s.dismissEdge);
  const acceptEdge = useApp((s) => s.acceptEdge);
  const toggleActionItem = useApp((s) => s.toggleActionItem);
  const entries = useApp((s) => s.entries);
  const actionItems = useApp((s) => s.actionItems);

  const deleteEntry = useApp((s) => s.deleteEntry);
  const playEntry = useApp((s) => s.playEntry);
  const playingEntryId = useApp((s) => s.playingEntryId);
  const [armed, setArmed] = useState(false);
  const armTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const customTypes = useApp((s) => s.customTypes);
  const addQuestion = useApp((s) => s.addQuestion);
  const dismissQuestion = useApp((s) => s.dismissQuestion);
  const resolveEntry = useApp((s) => s.resolveEntry);
  const reopenEntry = useApp((s) => s.reopenEntry);
  const [resolving, setResolving] = useState(false);
  const [resolutionDraft, setResolutionDraft] = useState('');
  const [probing, setProbing] = useState<string | null>(null);
  const [selection, setSelection] = useState<Span | null>(null);
  const [proposed, setProposed] = useState<Edge[]>([]);
  const [children, setChildren] = useState<Entry[]>([]);

  useEffect(() => {
    if (!id) return;
    void getBridge().listProposedEdges(id).then(setProposed);
    void getBridge().listChildren(id).then(setChildren);
  }, [id, entries]);

  useEffect(() => () => {
    if (armTimer.current) clearTimeout(armTimer.current);
  }, []);

  if (!entry) return null;

  const items = actionItems.filter((a) => a.entryId === entry.id);
  const probes = invokedProbes(entry, resolveTypes(customTypes));
  const other = (edge: Edge) => entries.get(edge.entryA === entry.id ? edge.entryB : edge.entryA);

  return (
    <aside className={styles.sheet}>
      <header className={styles.header}>
        <div className={styles.meta}>
          {dateFmt.format(new Date(entry.createdAt))}
          {' · '}
          {Math.round(entry.durationMs / 1000)}s
          {entry.audioPath === null && ' · typed'}
          {entry.localOnly && ' · local only'}
        </div>
        <div className={styles.headerActions}>
          {entry.audioPath !== null && playingEntryId !== entry.id && (
            <button type="button" className={styles.headerAction} onClick={() => playEntry(entry.id)}>
              play
            </button>
          )}
          {/* Two-step: deleting a recording you can't re-make deserves a beat. */}
          <button
            type="button"
            className={armed ? styles.deleteArmed : styles.headerAction}
            onClick={() => {
              if (!armed) {
                setArmed(true);
                armTimer.current = setTimeout(() => setArmed(false), 3000);
                return;
              }
              void deleteEntry(entry.id).then(close);
            }}
          >
            {armed ? 'delete — press again' : 'delete'}
          </button>
          <button type="button" className={styles.close} onClick={close} aria-label="Close">
            esc
          </button>
        </div>
      </header>

      <div className={styles.columns}>
        {/* Transcript first and largest. Nothing renders above it (§6.1). */}
        <article
          className={`${styles.transcript} selectable`}
          onMouseUp={(ev) => {
            const sel = window.getSelection();
            if (!sel || sel.isCollapsed || !entry) return setSelection(null);
            const host = ev.currentTarget;
            if (!host.contains(sel.anchorNode)) return setSelection(null);
            const pre = document.createRange();
            pre.selectNodeContents(host);
            pre.setEnd(sel.getRangeAt(0).startContainer, sel.getRangeAt(0).startOffset);
            const start = pre.toString().length;
            const end = start + sel.toString().length;
            if (end - start < 12) return setSelection(null);
            // Facet 3, exactly rather than by inference: this span, not the entry.
            const borrowed = entry.spans.some((sp) => sp.attributed && start < sp.end && end > sp.start);
            setSelection(borrowed || probes.length === 0 ? null : { start, end, attributed: false });
          }}
        >
          {segments(entry.transcript, entry.spans).map((seg, i) => (
            <span key={i} className={seg.attributed ? styles.attributed : undefined}>
              {seg.text}
            </span>
          ))}
        </article>

        {/* Secondary column, smaller and dimmer, so the tidy version never wins (§1.1). */}
        <div className={styles.side}>
          {entry.summary && <p className={styles.summary}>{entry.summary}</p>}
          <p className={styles.type}>{typeLabel(entry, resolveTypes(customTypes))}</p>
        </div>
      </div>

      {items.length > 0 && (
        <section className={styles.tasks}>
          {items.map((item) => (
            <label key={item.id} className={styles.task}>
              <input
                type="checkbox"
                checked={item.done}
                onChange={() => void toggleActionItem(item.id)}
              />
              <span className={item.done ? styles.taskDone : undefined}>{item.text}</span>
            </label>
          ))}
        </section>
      )}

      {entry.resolved && entry.resolutionText && (
        <section className={styles.resolution}>
          <span className={styles.resolutionLabel}>resolved</span>
          <p className={styles.resolutionText}>{entry.resolutionText}</p>
          {/* Reopening is offered, never automatic — a score should not overturn
              a conclusion you reached (§6.3). */}
          <button
            type="button"
            className={styles.resolveLink}
            onClick={() => void reopenEntry(entry.id)}
          >
            reopen
          </button>
        </section>
      )}

      {!entry.resolved &&
        (resolving ? (
          <section className={styles.resolution}>
            <span className={styles.resolutionLabel}>where did you land?</span>
            <textarea
              className={`${styles.resolutionField} selectable`}
              rows={2}
              autoFocus
              value={resolutionDraft}
              placeholder="in your own words — a future entry gets tested against this"
              onChange={(e) => setResolutionDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  e.stopPropagation();
                  setResolving(false);
                  return;
                }
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  const text = resolutionDraft.trim();
                  if (!text) return;
                  void resolveEntry(entry.id, text).then(() => {
                    setResolving(false);
                    setResolutionDraft('');
                  });
                }
              }}
            />
            <div className={styles.resolveActions}>
              <button type="button" className={styles.resolveLink} onClick={() => setResolving(false)}>
                esc
              </button>
              <button
                type="button"
                className={styles.resolveSave}
                disabled={!resolutionDraft.trim()}
                onClick={() => {
                  const text = resolutionDraft.trim();
                  if (!text) return;
                  void resolveEntry(entry.id, text).then(() => {
                    setResolving(false);
                    setResolutionDraft('');
                  });
                }}
              >
                Keep it
              </button>
            </div>
          </section>
        ) : (
          <button type="button" className={styles.resolveOpen} onClick={() => setResolving(true)}>
            landed on something?
          </button>
        ))}

      {children.length > 0 && (
        <section className={styles.stack}>
          {children.map((child) => (
            <div key={child.id} className={styles.layer}>
              <span className={styles.layerDate}>
                {dateFmt.format(new Date(child.createdAt))}
              </span>
              <span className={styles.layerLabel}>
                {child.resolutionText ? 'resolved' : 'recorded'}
              </span>
              <span className={styles.layerText}>{child.resolutionText ?? child.title}</span>
            </div>
          ))}
        </section>
      )}

      <button type="button" className={styles.analysisToggle} onClick={toggleAnalysis}>
        {analysisOpen ? 'hide analysis' : 'analysis'}
      </button>

      {analysisOpen && (
        <section className={styles.analysis}>
          {questions.map((q) => (
            <div key={q.id} className={q.dismissed ? styles.questionDismissed : styles.question}>
              {q.span && (
                <blockquote className={styles.quoted}>
                  {entry.transcript.slice(q.span.start, q.span.end)}
                </blockquote>
              )}
              <p className={styles.questionText}>{q.text}</p>
              <div className={styles.provider}>
                <span>{q.providerName}</span>
                {q.dismissed ? (
                  <span className={styles.dismissedTag}>dismissed</span>
                ) : (
                  !q.answered && (
                    <button
                      type="button"
                      className={styles.dismissQuestion}
                      onClick={() => void dismissQuestion(entry.id, q.id)}
                    >
                      dismiss
                    </button>
                  )
                )}
              </div>
              {!q.answered && !q.dismissed && (
                <p className={styles.answerHint}>
                  Press <kbd className={styles.kbd}>{hotkey}</kbd> to answer it out loud — it
                  becomes its own note, joined to this one, and gets a question of its own.
                </p>
              )}
            </div>
          ))}

          {proposed.map((edge) => {
            const target = other(edge);
            if (!target) return null;
            return (
              <div key={edge.id} className={styles.card}>
                <div className={styles.cardHead}>
                  <span className={styles.relation}>{edge.relation}</span>
                  <span className={styles.cardTitle}>{target.title}</span>
                </div>
                {edge.question && <p className={styles.cardQuestion}>{edge.question}</p>}
                <div className={styles.cardActions}>
                  <button type="button" onClick={() => void acceptEdge(edge.id)}>
                    keep
                  </button>
                  <button type="button" onClick={() => void dismissEdge(edge.id)}>
                    dismiss
                  </button>
                </div>
              </div>
            );
          })}

          {questions.length === 0 && proposed.length === 0 && (
            <p className={styles.nothing}>{silenceReason(entry)}</p>
          )}

          {/* Nothing is waiting, but this is the entry you came back to. */}
          {questions.length > 0 && questions.every((q) => q.answered || q.dismissed) && (
            <p className={styles.answerHint}>
              Nothing open here. Press <kbd className={styles.kbd}>{hotkey}</kbd> to say something
              else about it &mdash; it joins on as its own note.
            </p>
          )}

          {/* §11 rejected contention-as-a-button twice. Invocation lives on a
              selection instead: you ask about a sentence, which keeps the
              question specific and proves the span is yours before it fires. */}
          {selection && (
            <div className={styles.probes}>
              <button
                type="button"
                className={styles.ask}
                disabled={probing !== null}
                onClick={async () => {
                  setProbing('span');
                  try {
                    addQuestion(entry.id, await getBridge().askQuestion(entry.id, selection));
                    setSelection(null);
                    window.getSelection()?.removeAllRanges();
                  } finally {
                    setProbing(null);
                  }
                }}
              >
                {probing ? 'thinking…' : 'ask about this'}
              </button>
              <span className={styles.askHint}>
                {entry.transcript.slice(selection.start, selection.start + 34).trim()}…
              </span>
            </div>
          )}

          <div className={styles.connectRow}>
            <button
              type="button"
              className={styles.connect}
              onClick={() => setConnectSource(entry.id)}
            >
              connect to…
            </button>
            <span className={styles.connectHint}>
              or press <kbd className={styles.kbd}>C</kbd>, or drag the handle on the entry itself
            </span>
          </div>
        </section>
      )}
    </aside>
  );
}
