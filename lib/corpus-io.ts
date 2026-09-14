'use client';

import type { Entry } from '@/lib/types';

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
