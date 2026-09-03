'use client';

import type { Edge, Entry, Question } from '@/lib/types';

const stamp = () => new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');

function save(filename: string, mime: string, body: string): void {
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
export function exportTranscripts(entries: Entry[]): void {
  const ordered = [...entries].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  const parts = ordered.map((e) => {
    const head = `## ${e.title}\n\n_${dateFmt.format(new Date(e.createdAt))}_\n\n${e.transcript}\n`;
    const tail = e.summary ? `\n> generated summary: ${e.summary}\n` : '';
    const res = e.resolutionText ? `\n> resolved: ${e.resolutionText}\n` : '';
    return head + tail + res;
  });
  save(`transcripts-${stamp()}.md`, 'text/markdown', `# Transcripts\n\n${parts.join('\n---\n\n')}`);
}

/** Everything needed to restore, including what the Markdown export drops. */
export function exportJson(entries: Entry[], edges: Edge[], questions: Question[]): void {
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

/**
 * Uploaded files are untrusted input, so shape is checked before anything is
 * handed to the bridge — a bad file should say so, not half-load a corpus.
 */
export function parseImport(text: string): ParsedImport | { error: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { error: 'not valid JSON' };
  }
  if (typeof raw !== 'object' || raw === null) return { error: 'not a corpus file' };
  const o = raw as Record<string, unknown>;
  if (!Array.isArray(o.entries)) return { error: 'no entries in that file' };

  const entries = o.entries.filter(
    (e): e is Entry =>
      typeof e === 'object' && e !== null && typeof (e as Entry).id === 'string' &&
      typeof (e as Entry).transcript === 'string',
  );
  if (entries.length === 0) return { error: 'no usable entries in that file' };

  const edges = Array.isArray(o.edges)
    ? o.edges.filter((e): e is Edge => typeof e === 'object' && e !== null && typeof (e as Edge).id === 'string')
    : [];
  const questions = Array.isArray(o.questions)
    ? o.questions.filter(
        (q): q is Question =>
          typeof q === 'object' && q !== null && typeof (q as Question).entryId === 'string',
      )
    : [];

  return { entries, edges, questions };
}
