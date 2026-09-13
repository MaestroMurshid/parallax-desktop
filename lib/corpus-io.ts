'use client';

import type { ActionItem, Edge, Entry, Question, Span } from '@/lib/types';

const stamp = () => new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');

/** A browser download. Only the mock uses it: in the app a download from the
 *  webview saved silently to Downloads, so exports there go through a dialog. */
export function save(filename: string, mime: string, body: string): void {
  const url = URL.createObjectURL(new Blob([body], { type: mime }));
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

const dateFmt = new Intl.DateTimeFormat('en-GB', {
  day: 'numeric',
  month: 'long',
  year: 'numeric',
});

/**
 * §2 — the transcript is the record, so an export is the transcripts. The
 * summary goes underneath and marked as generated, never above and never
 * instead.
 */
export function transcriptsMarkdown(entries: Entry[]): string {
  const ordered = [...entries].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  const parts = ordered.map((e) => {
    const head = `## ${e.title}\n\n_${dateFmt.format(new Date(e.createdAt))}_\n\n${e.transcript}\n`;
    const tail = e.summary ? `\n> generated summary: ${e.summary}\n` : '';
    const res = e.resolutionText ? `\n> resolved: ${e.resolutionText}\n` : '';
    return head + tail + res;
  });
  return `# Transcripts\n\n${parts.join('\n---\n\n')}`;
}

/** The mock's stand-in for the archive: a browser cannot zip without a
 *  library, and this shape is what the app's upload still reads. */
export function saveCorpusJson(entries: Entry[], edges: Edge[], questions: Question[]): void {
  const body = JSON.stringify(
    { version: 1, exportedAt: new Date().toISOString(), entries, edges, questions },
    null,
    2,
  );
  save(`parallax-${stamp()}.json`, 'application/json', body);
}

export interface ParsedImport {
  entries: Entry[];
  edges: Edge[];
  questions: Question[];
}

// ---------------------------------------------------------------------------
// Validation. An uploaded file is untrusted input; every field the app
// actually reads is checked by type, not just presence, and the first entry
// that fails refuses the whole file. The prior version filtered instead of
// rejecting: a file of ten entries where nine were malformed silently
// imported the tenth, which could itself still be missing a field something
// downstream reads unconditionally — `spans`, unfiltered, is `undefined`
// where `EntryView`'s `segments()` calls `.filter` on it and throws.
// ---------------------------------------------------------------------------

const ROLES = new Set(['position', 'evidence', 'note']);
const REGISTERS = new Set(['live', 'neutral']);
const RELATIONS = new Set([
  'contradicts', 'same move', 'returns to', 'questions', 'extends', 'example of', 'answers', 'related',
]);
const EDGE_STATUSES = new Set(['proposed', 'accepted', 'dismissed', 'manual']);

type Rec = Record<string, unknown>;

const str = (x: Rec, f: string) => (typeof x[f] === 'string' ? null : `${f} is missing or not a string`);
const num = (x: Rec, f: string) => (typeof x[f] === 'number' ? null : `${f} is missing or not a number`);
const bool = (x: Rec, f: string) => (typeof x[f] === 'boolean' ? null : `${f} is missing or not a boolean`);
const nullableStr = (x: Rec, f: string) =>
  x[f] === null || typeof x[f] === 'string' ? null : `${f} is missing or not a string or null`;
const oneOf = (x: Rec, f: string, allowed: Set<string>) =>
  typeof x[f] === 'string' && allowed.has(x[f] as string) ? null : `${f} is not a valid ${f}`;

function isSpan(v: unknown): v is Span {
  if (typeof v !== 'object' || v === null) return false;
  const s = v as Rec;
  return typeof s.start === 'number' && typeof s.end === 'number' && typeof s.attributed === 'boolean';
}

function isActionItem(v: unknown): v is ActionItem {
  if (typeof v !== 'object' || v === null) return false;
  const a = v as Rec;
  return (
    typeof a.id === 'string' &&
    typeof a.entryId === 'string' &&
    isSpan(a.span) &&
    typeof a.text === 'string' &&
    typeof a.done === 'boolean'
  );
}

/** Returns the id of anything that has a string `id`, so a bad entry can
 *  still be named in the error that refuses it. */
function idOf(v: unknown): string | null {
  if (typeof v !== 'object' || v === null) return null;
  const id = (v as Rec).id;
  return typeof id === 'string' ? id : null;
}

/** Every field `spans` (an array) and everything else the app reads off an
 *  Entry (§7, EntryView, classification.ts) — not just `id`/`transcript`,
 *  which is all the filter this replaces used to check. Assumes the id
 *  itself has already been validated by the caller. */
function entryProblem(x: Rec): string | null {
  return (
    str(x, 'transcript') ||
    str(x, 'createdAt') ||
    num(x, 'x') ||
    num(x, 'y') ||
    nullableStr(x, 'audioPath') ||
    nullableStr(x, 'parentEdge') ||
    nullableStr(x, 'answersQuestionId') ||
    oneOf(x, 'role', ROLES) ||
    oneOf(x, 'register', REGISTERS) ||
    str(x, 'typeId') ||
    bool(x, 'resolved') ||
    nullableStr(x, 'resolutionText') ||
    str(x, 'title') ||
    nullableStr(x, 'summary') ||
    num(x, 'durationMs') ||
    (Array.isArray(x.fingerprint) && x.fingerprint.every((n) => typeof n === 'number')
      ? null
      : 'fingerprint is missing or not an array of numbers') ||
    bool(x, 'unfinished') ||
    bool(x, 'localOnly') ||
    (Array.isArray(x.spans) && x.spans.every(isSpan) ? null : 'spans is missing or not an array of spans') ||
    (Array.isArray(x.actionItems) && x.actionItems.every(isActionItem)
      ? null
      : 'actionItems is missing or not an array of action items') ||
    (x.isSample === undefined || typeof x.isSample === 'boolean' ? null : 'isSample is not a boolean')
  );
}

function edgeProblem(x: Rec): string | null {
  return (
    str(x, 'entryA') ||
    str(x, 'entryB') ||
    oneOf(x, 'relation', RELATIONS) ||
    nullableStr(x, 'question') ||
    oneOf(x, 'status', EDGE_STATUSES) ||
    str(x, 'createdAt')
  );
}

function questionProblem(x: Rec): string | null {
  return (
    str(x, 'entryId') ||
    str(x, 'text') ||
    (x.span === null || isSpan(x.span) ? null : 'span is not a span or null') ||
    bool(x, 'answered') ||
    bool(x, 'dismissed') ||
    str(x, 'providerName') ||
    str(x, 'createdAt')
  );
}

/**
 * Checks one array of records: every item must have a unique, non-empty id
 * and pass `problem`. Returns the first failure, naming the offending id, or
 * null when the whole array is usable. Mirrors `db::import::validate` on the
 * Rust side — an id collision or a missing id is a broken file, found out
 * before anything is written rather than partway through.
 */
function firstError<T>(
  noun: string,
  items: T[],
  problem: (x: Rec) => string | null,
): string | null {
  const seen = new Set<string>();
  for (const item of items) {
    const id = idOf(item);
    if (!id || id.trim() === '') return `an ${noun} in the import has no id`;
    if (seen.has(id)) return `the import names ${noun} ${id} twice`;
    seen.add(id);
    const bad = problem(item as Rec);
    if (bad) return `${noun} ${id} ${bad}`;
  }
  return null;
}

/**
 * Uploaded files are untrusted input, so shape is checked before anything is
 * handed to the bridge — a bad file should say so, not half-load a corpus.
 * Any invalid entry, edge or question refuses the whole file; partial import
 * is the defect (a), not a feature to preserve.
 */
export function parseImport(text: string): ParsedImport | { error: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { error: 'not valid JSON' };
  }
  if (typeof raw !== 'object' || raw === null) return { error: 'not a corpus file' };
  const o = raw as Rec;
  if (!Array.isArray(o.entries)) return { error: 'no entries in that file' };
  if (o.entries.length === 0) return { error: 'no entries in that file' };
  if (o.edges !== undefined && !Array.isArray(o.edges)) return { error: 'edges is not an array' };
  if (o.questions !== undefined && !Array.isArray(o.questions)) return { error: 'questions is not an array' };

  const edges = (o.edges as unknown[] | undefined) ?? [];
  const questions = (o.questions as unknown[] | undefined) ?? [];

  const entryError = firstError('entry', o.entries, entryProblem);
  if (entryError) return { error: entryError };
  const edgeError = firstError('edge', edges, edgeProblem);
  if (edgeError) return { error: edgeError };
  const questionError = firstError('question', questions, questionProblem);
  if (questionError) return { error: questionError };

  // An answer whose parent is nowhere is also refused (mirrors
  // `db::import::validate`), but whether "nowhere" is true depends on the
  // import mode — a merge may still have the parent already in the corpus —
  // and the mode is only chosen after this parse. That half of the check
  // runs in MockBridge.importCorpus, which knows both.

  return {
    entries: o.entries as Entry[],
    edges: edges as Edge[],
    questions: questions as Question[],
  };
}
