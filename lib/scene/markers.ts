/**
 * §5.3: unfinished is detected by **regex, not a model**. The user's own hedges
 * are a reliable signal and a model here would produce confident false
 * positives on exactly the entries where being wrong is least recoverable.
 *
 * The visual treatment is open: the dashed stroke it drove belonged to the
 * blob canvas, and the typographic one has no replacement yet.
 */
const HEDGES = [
  /\bidk\b/i,
  /\bi don'?t know\b/i,
  /\bi'?m not sure\b/i,
  /\bnot sure what\b/i,
  /\bi don'?t wanna say\b/i,
  /\bi don'?t want to say\b/i,
  /\bcan'?t quite\b/i,
  /\bsomething like that\b/i,
  /\bor whatever\b/i,
  /\bhaven'?t worked out\b/i,
  /\bstill figuring\b/i,
  /\bmaybe\?/i,
];

export function detectUnfinished(transcript: string): boolean {
  return HEDGES.some((re) => re.test(transcript));
}
