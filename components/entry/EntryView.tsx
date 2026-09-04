'use client';

import { useEffect, useRef, useState } from 'react';
import { getBridge } from '@/lib/bridge';
import { invokedProbes, resolveTypes, typeLabel } from '@/lib/scene/classification';
import { useApp } from '@/lib/store';
import type { Edge, Entry, Span } from '@/lib/types';
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

export default function EntryView({ hotkey }: { hotkey: string }) {
  const id = useApp((s) => s.selectedEntryId);
  const entry = useApp((s) => (id ? s.entries.get(id) : undefined));
  const question = useApp((s) => (id ? s.questions.get(id) : undefined));
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
  const setQuestion = useApp((s) => s.setQuestion);
  const resolveEntry = useApp((s) => s.resolveEntry);
  const reopenEntry = useApp((s) => s.reopenEntry);
  const [resolving, setResolving] = useState(false);
  const [resolutionDraft, setResolutionDraft] = useState('');
  const [probing, setProbing] = useState<string | null>(null);
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
        <article className={`${styles.transcript} selectable`}>
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
          {question && (
            <div className={styles.question}>
              <p className={styles.questionText}>{question.text}</p>
              {question.span && (
                <blockquote className={styles.quoted}>
                  {entry.transcript.slice(question.span.start, question.span.end)}
                </blockquote>
              )}
              <div className={styles.provider}>{question.providerName}</div>
              {!question.answered && (
                <p className={styles.answerHint}>
                  Press <kbd className={styles.kbd}>{hotkey}</kbd> to answer it out loud — the
                  answer becomes a layer on this entry, not a new note.
                </p>
              )}
            </div>
          )}

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

          {!question && proposed.length === 0 && (
            <p className={styles.nothing}>Nothing proposed for this entry.</p>
          )}

          {/* Heavy tier, invoked only (§3.6). Which probe fits is the model's
              call — a menu of techniques asks the wrong person to choose. */}
          {probes.length > 0 && (
            <div className={styles.probes}>
              <button
                type="button"
                className={styles.ask}
                disabled={probing !== null}
                onClick={async () => {
                  setProbing('auto');
                  try {
                    setQuestion(entry.id, await getBridge().askQuestion(entry.id));
                  } finally {
                    setProbing(null);
                  }
                }}
              >
                {probing ? 'thinking…' : 'ask it something'}
              </button>
              <span className={styles.askHint}>it picks what this entry needs</span>
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
