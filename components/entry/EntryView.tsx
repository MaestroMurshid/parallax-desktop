'use client';

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { getBridge } from '@/lib/bridge';
import { hasOwnSpan, invokedProbes, probeLabel, resolveTypes, slotFor } from '@/lib/scene/classification';
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

/** The move a question made, named for a person: Rust records it as the tail
 *  of `providerName` ("qwen3-4b-q4 · counterexample"). */
function tacticOf(q: Question): string | undefined {
  return probeLabel(q.providerName.split(' · ').pop() ?? '');
}

/** The target renders in the letterform the canvas gives it, so the panel
 *  speaks the same vocabulary rather than flattening everything to one face. */
function letterform(target: Entry): CSSProperties {
  const slot = slotFor(target);
  if (!slot) return {};
  return {
    fontFamily: slot.family === 'mono' ? 'var(--font-mono)' : 'var(--font-serif)',
    fontWeight: slot.weight,
    letterSpacing: `${slot.tracking}px`,
  };
}

/**
 * §5.3's best decision is currently invisible: a felt entry and a broken one
 * say the same eleven words. Name the reason, and the guardrail becomes the
 * feature rather than reading as a failure.
 */
function silenceReason(entry: Entry, liveRegister: boolean): string {
  if (entry.role === 'note') return 'A note — kept as written.';
  if (!hasOwnSpan(entry)) return "Every word here is someone else's. Nothing of yours to push on.";
  // Register first: evidence opens on its own now, so when a live one is quiet
  // the reason is the register, not the role.
  if (liveRegister && entry.register === 'live')
    return 'Left alone — this one reads as live. Select a sentence to take it on anyway.';
  // A typed note's duration is read off its word count, so it is short rather
  // than brief — and nothing was said, which the spoken wording claims.
  if (entry.durationMs < 10_000) {
    return entry.audioPath === null
      ? 'Short enough to stand as written, not interrogated.'
      : 'Under ten seconds — said once, not interrogated.';
  }
  return 'Nothing proposed for this entry.';
}

export default function EntryView({
  hotkey,
  liveRegister,
}: {
  hotkey: string;
  liveRegister: boolean;
}) {
  const id = useApp((s) => s.selectedEntryId);
  const entry = useApp((s) => (id ? s.entries.get(id) : undefined));
  const questions = useApp((s) => (id ? s.questions.get(id) : undefined)) ?? EMPTY;
  const analysisOpen = useApp((s) => s.analysisOpen);
  const thinking = useApp((s) => (id ? s.enriching.has(id) : false));
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
  const openEntry = useApp((s) => s.openEntry);
  const startRecording = useApp((s) => s.startRecording);
  const resolveEntry = useApp((s) => s.resolveEntry);
  const reopenEntry = useApp((s) => s.reopenEntry);
  const [resolving, setResolving] = useState(false);
  const [resolutionDraft, setResolutionDraft] = useState('');
  const correctTranscript = useApp((s) => s.correctTranscript);
  const [correcting, setCorrecting] = useState(false);
  const [correctionDraft, setCorrectionDraft] = useState('');
  const [savingCorrection, setSavingCorrection] = useState(false);
  const upsertEntry = useApp((s) => s.upsertEntry);
  const [registerBusy, setRegisterBusy] = useState(false);
  const [registerError, setRegisterError] = useState(false);
  const [typeBusy, setTypeBusy] = useState(false);
  const [typeError, setTypeError] = useState(false);
  const setOverlay = useApp((s) => s.setOverlay);
  const [probing, setProbing] = useState<string | null>(null);
  const [askError, setAskError] = useState<string | null>(null);
  const [selection, setSelection] = useState<Span | null>(null);
  const [selectAt, setSelectAt] = useState<{ x: number; y: number } | null>(null);
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

  // Correction is per-note and never survives the panel moving to another one:
  // a draft left open would otherwise be shown over, and saved onto, whichever
  // note you opened next.
  useEffect(() => {
    setCorrecting(false);
    setCorrectionDraft('');
    setAskError(null);
  }, [id]);

  // A note captured before the reasoning model landed never got a pass, and
  // nothing retried it (§9.4 lets the model arrive late, which had been letting
  // it arrive never). Opening the note is when someone is looking at it, so it
  // is when the gap is worth closing. Rust decides whether there is anything to
  // do; this only asks.
  useEffect(() => {
    if (!id) return;
    void getBridge()
      .ensureEnriched(id)
      .then((started) => {
        if (started) useApp.getState().setEnriching(id, true);
      })
      .catch(() => {
        // No model, or nothing to do. The note reads the same either way.
      });
  }, [id]);

  if (!entry) return null;

  const items = actionItems.filter((a) => a.entryId === entry.id);
  const probes = invokedProbes(entry, resolveTypes(customTypes));
  // Which child answered which question is not stored yet, so they pair in the
  // order both were made. Right for the common case of one question, one answer.
  const answeredIds = questions.filter((q) => q.answered).map((q) => q.id);
  const answerFor = (qid: string): Entry | undefined => children[answeredIds.indexOf(qid)];
  // The newest question still open; failing that the newest answered one, so
  // an answer stays one click away.
  const newestFirst = [...questions].reverse();
  const shown =
    newestFirst.find((q) => !q.answered && !q.dismissed) ?? newestFirst.find((q) => q.answered) ?? null;
  const canAsk = probes.length > 0;

  /** Another move on the same note. Asked before the current one is dismissed,
   *  so a failed ask leaves the question you had rather than none; Rust offers
   *  only moves this note has not had, the current one included. */
  const askAnother = async (current: Question | null) => {
    setProbing('another');
    setAskError(null);
    try {
      const next = await getBridge().askQuestion(entry.id, null);
      if (current && !current.answered && !current.dismissed) {
        await dismissQuestion(entry.id, current.id);
      }
      addQuestion(entry.id, next);
    } catch (e) {
      setAskError(e instanceof Error ? e.message : 'That did not work -- try again.');
    } finally {
      setProbing(null);
    }
  };
  const other = (edge: Edge) => entries.get(edge.entryA === entry.id ? edge.entryB : edge.entryA);

  return (
    <aside className={styles.sheet}>
      <header className={styles.header}>
        <div className={styles.meta}>
          {dateFmt.format(new Date(entry.createdAt))}
          {' · '}
          {Math.round(entry.durationMs / 1000)}s
          {entry.audioPath === null && ' · typed'}
          {/* The title, role and type on screen right now are placeholders
              derived from the words; saying so beats letting them read as the
              model's answer. */}
          {thinking && (
            <span className={styles.thinking} role="status">
              <span className={styles.thinkingDot} aria-hidden />
              reading it back
            </span>
          )}
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
        {/* Transcript first and largest. Nothing renders above it (§6.1).
            Read-only unless a correction is deliberately opened: the note is
            the verbatim record of what was said, and the one edit it takes is
            fixing what the transcription misheard. */}
        {correcting ? (
          <div className={styles.correction}>
            <p className={styles.correctionNote}>
              Correcting what the transcription <em>heard</em>. These are your words as you said
              them — fix the misheard ones, don&rsquo;t rewrite the thought.
            </p>
            <textarea
              className={`${styles.correctionField} selectable`}
              autoFocus
              rows={Math.min(18, Math.max(6, Math.ceil(correctionDraft.length / 72)))}
              value={correctionDraft}
              spellCheck
              onChange={(e) => setCorrectionDraft(e.target.value)}
              onKeyDown={(e) => {
                // Escape is stopped here or the window handler closes the whole
                // overlay (app/page.tsx) and takes the draft with it.
                if (e.key === 'Escape') {
                  e.stopPropagation();
                  setCorrecting(false);
                  setCorrectionDraft('');
                }
                // Deliberately no Enter-to-save, unlike the resolution field:
                // speech runs to paragraphs and a newline is a legitimate part
                // of a transcript, so Enter has to mean Enter here.
              }}
            />
            <p className={styles.correctionCost}>
              Highlights and questions re-find their own words. Any that no longer appear stop
              being highlighted — the questions themselves are kept.
            </p>
            <div className={styles.resolveActions}>
              <button
                type="button"
                className={styles.resolveLink}
                onClick={() => {
                  setCorrecting(false);
                  setCorrectionDraft('');
                }}
              >
                esc
              </button>
              <button
                type="button"
                className={styles.resolveSave}
                // Unchanged is not a correction, and blank is a delete wearing
                // one's clothes — Rust refuses it either way.
                disabled={
                  savingCorrection ||
                  !correctionDraft.trim() ||
                  correctionDraft === entry.transcript
                }
                onClick={() => {
                  setSavingCorrection(true);
                  void correctTranscript(entry.id, correctionDraft)
                    .then(() => {
                      setCorrecting(false);
                      setCorrectionDraft('');
                    })
                    .finally(() => setSavingCorrection(false));
                }}
              >
                {savingCorrection ? 'saving…' : 'Save correction'}
              </button>
            </div>
          </div>
        ) : (
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
            if (end - start < 12) {
              setSelectAt(null);
              return setSelection(null);
            }
            const rect = sel.getRangeAt(0).getBoundingClientRect();
            setSelectAt({ x: rect.left + rect.width / 2, y: rect.top });
            // Facet 3, exactly rather than by inference: this span, not the entry.
            const borrowed = entry.spans.some((sp) => sp.attributed && start < sp.end && end > sp.start);
            const ok = !borrowed && probes.length > 0;
            if (!ok) setSelectAt(null);
            setSelection(ok ? { start, end, attributed: false } : null);
          }}
        >
          {segments(entry.transcript, entry.spans).map((seg, i) => (
            <span key={i} className={seg.attributed ? styles.attributed : undefined}>
              {seg.text}
            </span>
          ))}
        </article>
        )}

        {/* Secondary column, smaller and dimmer, so the tidy version never wins (§1.1). */}
        <div className={styles.side}>
          {entry.summary && <p className={styles.summary}>{entry.summary}</p>}
          {/* A picker, not a label: the editor's own hint has always said
              "manual" tags entries yourself, and this is the door that opens.
              The live-register suffix (`typeLabel`'s " · live") is display
              only and never a value this can select, so it is shown beside
              the picker rather than folded into an option. */}
          <div className={styles.typeRow}>
            <select
              className={styles.typeSelect}
              value={entry.typeId}
              disabled={typeBusy}
              onChange={(ev) => {
                const typeId = ev.target.value;
                setTypeBusy(true);
                setTypeError(false);
                void getBridge()
                  .setEntryType(entry.id, typeId)
                  .then(upsertEntry)
                  .catch(() => setTypeError(true))
                  .finally(() => setTypeBusy(false));
              }}
            >
              {resolveTypes(customTypes).map((t) => (
                <option key={t.id} value={t.id}>
                  {t.label}
                </option>
              ))}
            </select>
            {liveRegister && entry.register === 'live' && (
              <span className={styles.type}>· live</span>
            )}
          </div>
          {typeError && <p className={styles.type}>that did not save — try again</p>}

          {/* The user overruling the classifier on one note. Offered only while
              the facet is switched on, because with it off the answer changes
              nothing and the control would be a switch wired to nothing. */}
          {liveRegister && (
            <button
              type="button"
              className={styles.correctOpen}
              disabled={registerBusy}
              onClick={() => {
                setRegisterBusy(true);
                void getBridge()
                  .setRegister(entry.id, entry.register === 'live' ? 'neutral' : 'live')
                  .then(upsertEntry)
                  .catch(() => setRegisterError(true))
                  .finally(() => setRegisterBusy(false));
              }}
            >
              {entry.register === 'live' ? 'not personal — open it up' : 'personal — leave it alone'}
            </button>
          )}
          {registerError && (
            <p className={styles.type}>that did not save — try again</p>
          )}

          {/* §9 keeps SQLite authoritative, so this shows what a file would
              contain rather than offering a way to write one. */}
          <button type="button" className={styles.correctOpen} onClick={() => setOverlay('file')}>
            view as file
          </button>
          {/* Quiet, and named for the only thing it does. A prominent "edit"
              here would invite rewriting the thought, which is the one change
              a commonplace book cannot take.

              Withheld from a typed note: nothing misheard it, so the only edit
              it could offer is the rewrite the affordance exists to avoid. */}
          {!correcting && entry.audioPath !== null && (
            <button
              type="button"
              className={styles.correctOpen}
              onClick={() => {
                setCorrectionDraft(entry.transcript);
                setCorrecting(true);
              }}
            >
              misheard?
            </button>
          )}
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

      {selection && selectAt && (
        <button
          type="button"
          className={styles.askFloating}
          style={{ left: selectAt.x, top: selectAt.y }}
          disabled={probing !== null}
          onMouseDown={(e) => e.preventDefault()}
          onClick={async () => {
            setProbing('span');
            try {
              addQuestion(entry.id, await getBridge().askQuestion(entry.id, selection));
              setSelection(null);
              setSelectAt(null);
              window.getSelection()?.removeAllRanges();
            } finally {
              setProbing(null);
            }
          }}
        >
          {probing ? 'thinking…' : 'ask me about this'}
        </button>
      )}

      <button type="button" className={styles.analysisToggle} onClick={toggleAnalysis}>
        {analysisOpen ? 'hide analysis' : 'analysis'}
      </button>

      {analysisOpen && (
        <section className={styles.analysis}>
          {/* One question at a time: a stack of them read as a questionnaire
              rather than a question. Dismissed ones stay in the record (and in
              the file view); they are just not what you are shown. */}
          {shown && (
            <div key={shown.id} className={shown.answered ? styles.questionAnswered : styles.question}>
              {shown.span && (
                <blockquote className={styles.quoted}>
                  {entry.transcript.slice(shown.span.start, shown.span.end)}
                </blockquote>
              )}
              <p className={styles.questionText}>{shown.text}</p>
              <div className={styles.provider}>
                <span title={shown.providerName}>{tacticOf(shown) ?? shown.providerName}</span>
                {shown.answered ? (
                  <span className={styles.answeredTag}>answered</span>
                ) : (
                  <button
                    type="button"
                    className={styles.dismissQuestion}
                    disabled={probing !== null}
                    onClick={() => void dismissQuestion(entry.id, shown.id)}
                  >
                    dismiss
                  </button>
                )}
                {canAsk && (
                  <button
                    type="button"
                    className={styles.dismissQuestion}
                    disabled={probing !== null}
                    onClick={() => void askAnother(shown)}
                  >
                    {probing === 'another' ? 'thinking…' : 'ask another'}
                  </button>
                )}
              </div>
              {shown.answered && answerFor(shown.id) && (
                <button
                  type="button"
                  className={styles.answerLink}
                  onClick={() => openEntry(answerFor(shown.id)!.id)}
                >
                  <span className={styles.answerDate}>
                    {dateFmt.format(new Date(answerFor(shown.id)!.createdAt))}
                  </span>
                  <span className={styles.answerTitle}>{answerFor(shown.id)!.title}</span>
                </button>
              )}

              {!shown.answered && (
                <button
                  type="button"
                  className={styles.answerThis}
                  onClick={() => void startRecording(entry.id, shown.id)}
                >
                  answer this
                  <span className={styles.answerKey}>{hotkey}</span>
                </button>
              )}
            </div>
          )}
          {askError && <p className={styles.nothing}>{askError}</p>}

          {proposed.length > 0 && (
            <p className={styles.sectionLabel}>
              {proposed.length === 1 ? 'one connection proposed' : `${proposed.length} connections proposed`}
            </p>
          )}

          {proposed.map((edge) => {
            const target = other(edge);
            if (!target) return null;
            return (
              <div key={edge.id} className={styles.card}>
                <div className={styles.cardHead}>
                  <span className={styles.relation}>{edge.relation}</span>
                  <button
                    type="button"
                    className={styles.cardTitle}
                    style={letterform(target)}
                    onClick={() => openEntry(target.id)}
                  >
                    {target.title}
                  </button>
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

          {/* §5.3's reasons are all statements about a decision the model has
              already taken. While it is still deciding, none of them is true
              yet -- "kept as written" on an entry about to be classified as a
              position is the wrong thing to have said. */}
          {!shown && proposed.length === 0 && (
            <p className={styles.nothing}>{thinking ? 'Reading it back…' : silenceReason(entry, liveRegister)}</p>
          )}
          {/* Nothing on screen to re-ask from -- every question was dismissed,
              or none opened on its own -- so asking gets its own way in. */}
          {!shown && canAsk && !thinking && (
            <button
              type="button"
              className={styles.dismissQuestion}
              disabled={probing !== null}
              onClick={() => void askAnother(null)}
            >
              {probing === 'another' ? 'thinking…' : 'ask me a question'}
            </button>
          )}

          {/* Nothing is waiting, but this is the entry you came back to. */}
          {shown?.answered && proposed.length === 0 && (
            <p className={styles.answerHint}>
              Nothing open here. Press <kbd className={styles.kbd}>{hotkey}</kbd> to say something
              else about it. It joins on as its own note.
            </p>
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
