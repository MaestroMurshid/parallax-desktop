# Handoff — voice-first thinking tool

Working document. Everything here was arrived at over a long design conversation; where
something was considered and rejected, the reasoning is recorded, because the rejections
are load-bearing and will otherwise get re-proposed.

---

> **Status:** a UI mockup on the real stack now exists. **§17 is the current
> state** — the taxonomy in §3.6 has been recut and two rejections in §11 have
> been reversed. Read §15, §16 and §17 before trusting anything above them. This
> document is a working record, not a specification: where it and the build
> disagree, the build is usually right and the doc is out of date.

## 1. What this is

A background voice-capture app that becomes a place to think in. Global hotkey from
anywhere, ramble, filed automatically. Over time entries find each other, and the app can
draw a line between two of them, ask about one, or let you work something out further.

**The interlocutor is your past self, not the AI.** A note from March isn't trying to be
agreeable to your November position — it's the least sycophantic material available to
you, and it's sitting unread. The AI retrieves it, puts it next to something relevant, and
asks.

That framing is not a claim of AI neutrality. Choosing which entries to pair and what to
ask is already an interpretive act and will frequently be wrong. During the design
conversation the assistant misread the same note three times in a row, and each correction
came from the user. The whole system is built so that being wrong is cheap and recoverable:
questions instead of verdicts, every analytical claim anchored to a quoted span, every
proposed connection dismissible, nothing auto-applied.

### What it does — all of it, not one thing

| | |
|---|---|
| **Capture** | frictionless, voice, in the moment, no window to look at |
| **Connect** | across time, on argumentative shape as well as topic |
| **Deepen** | be asked about something you're still forming, not told |
| **Check** | what a claim rests on, where it stops, whether you're biased |
| **Surface** | what recurs, what you've said before in different words |
| **Preserve** | the record intact — no smoothing, no rewriting |

Earlier drafts of this doc compressed all of that into "it stops thoughts staying
unfinished." That's the strongest single thing it does, not the definition. Don't pitch
the whole product as one line; it loses the other five.

### What it refuses

- **Won't summarize over your words.** Summaries exist (tray labels, similarity math) and
  are never rendered above your transcript.
- **Won't answer.** It asks. Verdicts give you something to argue with instead of
  something to think about.
- **Won't chat.** No panel, no turn two, no persistent conversation.
- **Won't rewrite you.** See §11 on the GPT-rewrite failure.

Note it *does* organize — auto-placement, auto-connection, generated titles. An earlier
draft claimed "won't organize for you," which contradicts the design. Organization is
automated deliberately (§5.1).

### 1.1 Reading the brief

The task as stated:

> Build a desktop voice note-taking application that captures microphone audio, transcribes
> it locally, and intelligently enriches the transcription based on its content and
> available context. […] if a voice note contains an action item, the application could
> recognize it and represent it as a tickable task. This is only one example. The candidate
> is encouraged to explore other useful ways of organizing, enriching, and presenting voice
> notes.

**The interpretive move this design makes is on "available context."** Most readings will
take that as ambient signal — time of day, calendar, active window, location. This design
reads it as **the user's own prior recordings**. That is the thesis, and it's what makes
cross-time connection the centre of the product rather than a feature bolted on.

Mapping brief language onto what's here:

| brief | this design |
|---|---|
| captures microphone audio | §4 — hotkey-first, panel second, no window to look at |
| transcribes it locally | §9.0 — transcribe.cpp, local unconditionally, never optional |
| enriches based on **content** | classification (§3.6), generated titles (§5.2), move-vector extraction (§7.1), attributed-span separation (§7.3), action items (§1.2) |
| enriches based on **available context** | §7.2 — the background pass, deliberately excluding the last two weeks so every connection is *cross-time*. Prior notes as context. |
| beyond storing raw transcripts | typed relations (§5.4), threads and resolution stacks (§6), the question (§3) |
| organizing | frozen spatial placement, emergent territories (§5.1) |
| presenting | fingerprints, rings, markers, thread view (§5, §6) |

**Where this design deliberately declines, and why it should be said out loud.** "Enrich"
is read here as *add to*, never *replace*. The transcript is never rewritten, never
summarised over, never smoothed. That refusal is a product decision, not an omission, and
the reasoning is in §10 and §11 — the demonstrated failure of a GPT rewrite flattening
"warm and weird and nice" into something tidier, plus the CSCW 2025 finding that the most
automated assistance produced the worst outcomes while being the most preferred.

State this explicitly in the README. A reviewer expecting a summary field should find the
argument for its absence, not a gap.

**Summaries do exist and can be shown.** One is generated per entry anyway (§7.1) for tray
labels and similarity, so surfacing it is nearly free. Showing it is fine; letting it define
the product is not. Rules:

- **Never above the transcript.** A side column or a collapsed panel below — not a header.
- **Transcript keeps visual primacy:** primary column, full type size, normal contrast. The
  summary is secondary column, smaller, dimmer. If they look equally important the reader
  will take the tidy one and skip their own words, which is the ordering effect this design
  exists to prevent.
- **Copy and export default to the transcript.** The summary is a view, never the record.
- **Never generated for felt entries.** "Reflects on a relationship that ended" is worse
  than useless; it flattens the exact thing that made the writing worth keeping. Same
  suppression list as §3.2.
- Regenerating a summary is fine — unlike the question, it carries no stance, so there's no
  slot-machine problem.

### 1.2 Action items — hitting the stated example

The brief's own example is worth implementing, both because it's cheap and because not
doing it reads as not having read the brief.

An action item is an **inert type** in the sense of §3.6 — it never initiates a question,
never gets probed, never nags. It is enrichment in the purest form: recognised from content,
rendered usefully, transcript untouched.

- Detected during the same pass that classifies the entry.
- Rendered inside the entry view as a tickable checkbox, anchored to the span it came from,
  so ticking never edits the transcript.
- Collected into one lightweight global task list — which is also a clean, immediately
  legible demo surface that works with a near-empty corpus.
- **No canvas encoding.** §5.3 caps the blob at roughly three visual channels and they are
  spent. Tasks live in the entry and in the list, not on the map.
- Ticking is state on the span, not a mutation of the text. Same principle as resolution
  (§6.3): the record accumulates, nothing is overwritten.

This also demonstrates the type system doing real work — the same machinery that suppresses
probing on felt entries is what makes a task inert.

---

## 2. Why voice

- ~3× typing speed, so the corpus reaches useful volume in months rather than years.
- Speech is closer to actual idiolect than edited prose.
- Answering aloud is where you find out whether you have anything. The user's own note —
  *"I thought I had so much to say but when I wanted to actually articulate it, I had
  nothing"* — is that discovery happening. That's the mechanic working, not failing.
- Speech marks quotation reliably ("so this guy was saying", "I read somewhere that"),
  which makes §7.3 tractable.

This cuts against the entire voice-notes market. AudioPen, Voicenotes, Speakwise are all
compression engines: ramble in, tidy paragraph out. The digression is the content here.
**The raw transcript is the stored record, always.**

---

## 3. The AI's repertoire

This is the section previous drafts flattened into "asks one question." It is not one
question. There are distinct modes, they fire in different situations, and picking the
wrong one is the main way this product can hurt someone.

### 3.1 The modes

**A. Load-bearing assumption**
> "This rests on X being the case. Is it?"

General-purpose, low-risk. You can't answer without going further into your own position.
This is the default for the automatic post-recording question.

**B. Boundary-finding**
> "Does this hold when Z?"

Not a refutation — a request for the edges, which is usually where the real idea lives.
Also low-risk.

**C. Disconfirming case**
> "What would make you drop this?"

Slightly sharper. Good on confident claims, bad on tentative ones.

**D. Steelman-then-probe**

State the argument better than the user did, *then* push. Being understood before being
challenged removes almost all the defensive reflex. Costs more tokens; worth it on
anything the user has returned to more than once.

**E. Münchhausen chain** — see §3.3 for full gating

Ask why. Ask why of the answer. Three or four steps and it bottoms out. Bounded by
construction, which is why it's usable — it has a stopping condition rather than running
until the user gets bored.

**F. Feynman / explain-it-simply**

Only where there is a *concept to master*. Reference notes, things being learned. Useless
against a personal claim, actively grotesque against anything felt. The zettelkasten repo
found on GitHub (joshylchen) uses a "curious 12-year-old student" persona for all notes —
that's this mode applied indiscriminately, and it's the wrong default.

**G. Contradiction retrieval**

Not a generated question at all — a retrieval result. "You argued the opposite in March,"
with both spans quoted. The model has no opinion here, which is exactly why it can't
flatter. This is the strongest mode and the one to build first.

**H. Juxtaposition, silent**

Two entries placed next to each other, no question, no analysis. For emotional material.
From the conversation: the user wrote *"as long as I understand it, I don't have to suffer
from it"* and elsewhere *"I'm stabbed for what I lost."* Putting those two lines on a page
together does the entire job. A question underneath would ruin it.

**I. Inert**

Some entries never initiate anything: lists, admin, intent notes ("learn woodworking").
They remain embedded and can be *surfaced as neighbours* — an intent note from 2024
appearing next to a live 2026 ramble on the same subject is a real and good connection —
but they never generate a question and never nag. A list resurfaced *as a list* is
nagging, and one nag makes the app something you avoid opening.

### 3.2 Which mode fires, and when

Two separate triggers with different rules.

**Automatic (post-recording, once):** restricted to A, B, C, or nothing. Never E, never F,
never anything heavy. It fires on the newest entry only, while the thread is still live —
so old entries are safe by construction and nothing ever goes back to poke at something
recorded during a bad week.

Suppression: no automatic question if the entry classifies as felt, inert, or is under
~30 seconds. When in doubt, nothing. A silent app is recoverable; an app that probed
someone's grief is not.

**Invoked (user selects a passage, or opens a connection):** any mode. This is where E, D,
and F live. The user chooses to be challenged, so the risk of misfire is theirs.

Note the tension, and resolve it deliberately: the user rejected auto-routing by type
(§11), but the automatic question needs *some* gate or it will probe the wrong things. The
resolution is that classification never *selects* a heavy mode — it only *suppresses*.
Misclassification then costs a missing question rather than an intrusive one.

**G (contradiction) is not a mode selection at all.** It fires when the background pass
finds two entries in genuine tension, regardless of type — except that a felt entry in
tension with another felt entry gets H, not G.

### 3.3 Münchhausen — full gating

The user asked directly: where is this invoked? Not on a grocery list.

**Fires only when all of:**
- entry contains a claim with at least one supporting reason (a justification structure,
  not just an assertion and not just a description)
- entry type is claim or rant, never felt, never inert, never a source excerpt
- user invoked it, or the entry has been returned to ≥2 times (i.e. it's clearly live)
- not already run on this entry in the last N returns

**Never fires on:** lists, reminders, intent notes, book excerpts, anything classified
felt, anything under ~30s, anything where the model can't identify a reason-giving
structure.

**Behaviour:**
- Runs entirely on the model side. The trilemma is never rendered, never named, never
  explained to the user. No diagram of the three horns.
- Bounded to 3–4 steps. One question per step.
- Stops when the user stops answering. No follow-up, no "still there?" A friend who does
  that is fine; a notification that does it is a nag.
- **Never announces which horn was hit.** "Your reasoning is ungrounded" is *always* true
  and therefore says nothing. Reporting the trilemma is reporting nothing.
- The output is the terminus itself: "you stopped at X — is that where you meant to stop?"
  The value is *which* axiom, not *that there is one*.
- The terminus is not a failure state. Coherentism accepts a wide enough circle;
  infinitism defends the regress; foundationalism defends basic beliefs. If the UI frames
  bottoming-out as a defeat, this becomes a nihilism generator rather than a thinking tool.

**Why it's a good mechanic here:** it has a natural stopping condition, so it's a bounded
interaction rather than an open conversation, and every step is a question with no verdict
attached. Also it's already what the user writes about — regress and arbitrary stopping
points recur across their notes (moral objectivity, the microbes/hierarchy note). The tool
would be formalising something they already do.

**Chain output in the UI:** just a run of child entries in the thread. No new object type.
Each step is an entry with `parent_edge` set, same as any other answer.

### 3.4 Stance rules — apply to every mode

- **Push on reasoning, not conclusions.** "You're wrong about X" produces a rebuttal.
  "This holds if Y — is Y true?" produces development. Same disagreement, opposite outcome.
- **It may ask. It may not conclude.** A question has no position to attack, so the only
  thing available to push against is your own reasoning.
- **One thing per surfacing.** Five objections is an attack. One question is an invitation.
- **Scale to stakes.** Light forms for a live idea still forming; the harder forms only for
  things returned to repeatedly. Something said once doesn't deserve interrogation.
- **Third person about the entry, not the author.** "The entry treats X as settled," never
  "you believe X." The second slides into psychoanalysing the user, which is where
  invented personality claims come from.
- **Every analytical claim quotes a span.** Kills eloquent unanchored nonsense, makes the
  output checkable rather than authoritative.
- **No regenerate button.** Rerolling until you get an agreeable question is the echo
  chamber arriving through the back door.
- **Ignoring is free.** Escape leaves it. Not a decision, not a dismissal, no cost.

### 3.5 Failure signal

If the user finds themselves *explaining themselves to it*, the stance is wrong. If they
find themselves going quiet and then recording something longer than intended, it's
working.

### 3.6 Architecture — don't close this off

A–I above is not a fixed enum to hardcode. Two separate axes are in play and they scale
differently.

**Axis 1 — note types (what an entry *is*).** nodepad.space classifies every note into one
of 14: *claim, question, idea, task, entity, quote, reference, definition, opinion,
reflection, narrative, comparison, thesis, general.* That works there because their kanban
view groups by type — a list tolerates 14 rows.

On a canvas it doesn't. A blob carries roughly three visual encodings before it becomes
noise (§5.3). So:

- **Rendered types stay at 3 + inert:** claim / rant / felt, plus inert.
- **Stored types can be as fine as you like.** Keep the full classification as a field;
  render the collapse. Finer types remain available for filtering, search, and prompt
  construction without costing canvas legibility.

**Axis 2 — probe modes (what the AI *does*).** These can proliferate, but only inside risk
tiers. The real structure of A–I isn't nine things, it's four positions:

| tier | modes | may fire automatically? |
|---|---|---|
| **Silent** | H (juxtaposition), I (inert) | n/a — generates nothing |
| **Safe** | A (load-bearing), B (boundary), C (disconfirming) | yes |
| **Heavy** | D (steelman), E (Münchhausen), F (Feynman) | **no — invoked only** |
| **Retrieval** | G (contradiction) | yes — no prompt to go wrong |

Modes can be added freely *within* a tier. What must never be loose is the tier
assignment, because the asymmetry is brutal: a missing question costs nothing, a heavy
probe on an entry about someone's grief is unrecoverable.

**Modes are probably not a runtime switch at all.** Rather than having the model select
from an enum, give it the entry plus the stance rules of §3.4 — push on reasoning not
conclusions, ask don't conclude, one thing only, quote a span — and let it generate. A–I
then becomes a *description of what good output looks like*, used for writing and
evaluating the prompt, not for branching. That scales without limit: adding a mode is just
noticing a new shape in outputs you already like. **The gate stays hardcoded regardless.**
That's the part with teeth.

**User-defined types.** The user should be able to define their own type plus the prompt
the AI runs on it. E.g. a *self-improvement* type whose instruction is roughly "help me
move toward the goal in this entry" rather than probing its foundations. This is the main
extensibility surface and should exist from early on.

Requirements for a user-defined type:

```
name          short label
match         how entries get this type: manual tag, or a natural-language
              description the classifier matches against
prompt        what the AI does with it
tier          defaults to INVOKED-ONLY; user may opt it into automatic
render        optional — maps onto one of the 3 rendered types, or none
```

Two rules that must hold for user-defined types:

1. **Default to invoked-only.** A user-authored prompt firing automatically on a
   misclassified entry is the same failure as a heavy built-in mode. Let them opt in
   deliberately.
2. **Suppression is not overridable.** A user-defined type still cannot fire on entries
   classified felt, inert, or under ~30s. The user owns the prompt; they don't own the
   safety gate.

This also gives a graceful path for the modes we haven't thought of. If the built-in set
turns out to be missing something the user needs, they can write it themselves rather than
waiting for it — and the ones that get used a lot are the candidates for promotion into
the built-in set.

---

## 4. Capture

Global hotkey **starts recording immediately**; the panel appears second. This ordering
matters — panel-first means two actions and a moment looking at UI before speaking.
Recording-first means hit the key and talk while walking away from the screen.

**Recording state:** live equalizer bars (the Siri/Assistant idiom — bars bounce, symbol
stays put), elapsed time. Nothing else. No transcript on screen: reading your own words
back in the moment makes you self-edit next time. No buttons to look for.

**Same hotkey stops.** A separate key discards while recording. Discard belongs in the
recording state, not after — you know it's junk before you stop. No confirmation dialog,
but keep a ~60s undo window, because escape meaning "discard" while recording and "leave
it" when stopped is a muscle-memory trap.

**On stop:** bars freeze into the fingerprint of what was just said. Transcription runs
(~2s). Then one question appears in serif underneath. Hotkey again to answer by voice.

For long recordings the panel should release rather than hold — freeze, close, and let the
question surface on the entry later. Holding is only right for short ones.

**Typed entry** must exist as an alternative path. Nobody dictates a list. Typed entries
have no fingerprint, which becomes a free visual distinction.

---

## 5. The canvas

### 5.1 Placement

Auto-placed, auto-connected. Manual placement was designed and then cut: the overhead
would kill daily use, and PIM research (Bergman & Whittaker) is unambiguous that hand-
filing gets abandoned. Dragging exists as an occasional override, not an obligation.

**Positions are assigned once and frozen forever.** A new entry solves against a fixed
field: take its nearest neighbours by similarity, compute a target point from their
positions, walk outward until it finds empty space. Nothing already placed ever moves.

This is the single most important layout decision. Force-directed layout re-solves on every
insert, so nothing is ever where you left it, which is precisely why Obsidian's graph view
is universally admired and never used for thinking. Linear, deterministic, cheap — you are
not running a graph layout algorithm, you are placing one node.

**No strong neighbours →** land in open space at the edge, not at the centroid of
everything. Honest: this doesn't connect to anything yet. Over time you get visible
territories with unclaimed ground between them.

**Manual links don't move nodes.** A link is a statement about relation, not position.

**Consequence worth keeping:** position encodes *when*. Early entries hold the space they
claimed first; later ones fill in around them. Old clusters stay physically where they
were even as interests move elsewhere, so you can see the shape of a preoccupation you've
drifted out of, sitting where you left it.

### 5.2 Blobs

- **Size** = duration.
- **Title** = 3–4 words, generated, drawn from the user's own phrasing wherever possible.
  "stabbed for what I lost", not "reflection on loss". Titles are the primary identifier —
  you navigate by reading them. A canvas of unlabelled blobs is the Obsidian graph failure:
  pretty, unreadable.
- **Fingerprint** = 7–9 vertical rounded bars, downsampled from actual audio amplitude.
  Every recording looks different. 5 bars is too few — too many recordings look alike.
  Renders only above a zoom threshold.
- **Type in the edge treatment**, quietly, because classification is a guess:
  - crisp circle → claim
  - irregular blob path → rant
  - soft blurred fill, no stroke → felt
  Reads as texture at canvas scale; you notice a region feels blurry before consciously
  registering why. Irregularity means **unresolved**, not **angry** — see §11 on emotion
  encoding. If intensity is wanted it's already free in the fingerprint: dense loud speech
  gives tall uniform bars, halting speech gives gaps.

### 5.3 Markers

| marker | meaning |
|---|---|
| dashed stroke | left unfinished — from the user's own hedges ("idk", "I don't wanna say", "not sure what"). **Regex, not a model.** |
| concentric rings | number of returns; more rings, more layers |
| single solid outer ring | resolved (user-declared) |
| orange dot `#D85A30` | unanswered question — the only colour in the entire app |
| dimmed fill, no edges | connects to nothing |

Allocation principle: **type gets the quietest channel, pending question gets the loudest.**
Type is a guess and will sometimes be wrong, so it shouldn't shout. An unanswered question
is certain and actionable, so it earns the one saturated element.

No ticks, no green, nothing reading as completed or as a backlog item.

**Known clash:** the concentric thread ring competes visually with the playing-audio pulse
rings. Make the thread ring a partial arc if both can appear at once. Also, the soft "felt"
blob may vanish at low zoom having no stroke — may need a faint one at small sizes.

### 5.4 Edges

Thin lines, horizontal label at the midpoint, background knocked out behind the text.
Horizontal not rotated — steep-angle rotated text is unreadable, and Obsidian does the same
thing for the same reason.

**An edge requires a nameable relation.** *contradicts, same move, returns to, questions,
extends, example of, echoes.* If the best the model can produce is "related" or "same
subject," **draw nothing** — proximity already carries that.

This distinction was got wrong once during design and is worth stating flatly:
**nearness is cheap and can be fuzzy; an edge is an assertion and has to be earned.**
Three notes sitting in the same region because their vectors are close is fine. Three notes
joined by lines because they're all vaguely "tech" is the failure mode.

**Threshold high.** One confident wrong edge teaches the user to distrust all of them; a
missing edge is recoverable later when they write something adjacent. Tune against the
real corpus: take ~50 entries, inspect top pairs at various thresholds, find where they
stop being obviously right. The number is personal — someone writing on one topic
constantly needs a much higher bar than someone whose notes scatter.

**Run the move vector at a lower bar than topic.** Topical hits are common and cheap; move
matches are rare and meaningful.

**Thread edges render brighter and heavier** than proposed semantic edges. A thread edge is
a fact (the user recorded that answer to that entry); a semantic edge is a guess. Visual
weight should track confidence.

**Manual edge drawing must exist.** With a high threshold the app will miss real
connections. Drawing and naming one yourself is also the naming step the research says
carries the cognitive benefit.

### 5.5 LOD

Zooming out drops, in order: labels → fingerprints → edges → leaving blobs and sizes at
full-map view. ~20 blobs is the ceiling for readable titles at normal zoom.

Never load transcripts into JS for canvas rendering. Query SQLite for the viewport; the
canvas needs positions, sizes, ids, titles.

---

## 6. Entry view, threads, resolution

### 6.1 Entry view

Transcript verbatim, nothing above it. Analysis collapsed by default, opens on request —
so the user reads themselves before reading a reading of themselves. If analysis renders
first it colours how they read their own words.

Select a passage → one question. No chat box, no reply field, no turn two. Answer by
recording; the answer becomes a child entry.

Proposed edges sit *below* the transcript as dismissible cards. Never open themselves,
never animate in. Dismissals are training signal, not just UI.

### 6.2 Threads

**An answer is not a peer note.** It's a layer on the parent — it thickens the parent's
ring rather than spawning its own blob. Otherwise answering three questions triples the
node count with fragments that mean nothing standing alone.

A blob with three rings is instantly the thing you've thought hardest about, which is
information you currently have no way to see.

### 6.3 Resolution

User-declared. The AI never decides you're done thinking about something.

**Resolving requires stating what the resolution is** — not a flag. Partly because writing
it is where the thinking lands, mainly because a stated position is what a future entry can
be tested against. A bare "resolved" gives the app nothing.

**Reopening is proposed, never automatic.** A similarity score should not overturn a
conclusion. Flag that a new entry sits against a resolved position; the user decides
whether it's a real contradiction or a bad day.

**Never overwrite. Stack.** A thread reads:

```
Mar · recorded     the blog is where half-formed thinking goes;
                   the wiki is where it ends up once it's cooked
Mar · resolved     stream first, crystallise second
Apr · contradicts  noticed the opposite happening — raw thoughts
                   going into the wiki first
Jul · resolved     not about how finished it is; the wiki is what's
                   worth keeping, the blog is what's worth saying now
Sep · contradicts  maybe the split was never real and both halves
                   are one commonplace book
```

(That arc is real — paraphrased from Neil Mather's public wiki, §10.)

Two things visible in that stack without any analysis:

1. **The second break is a different kind from the first.** April challenges the *answer*;
   September challenges the *question*. If the model can distinguish those, "you're
   refining a distinction that may not hold" is a genuinely valuable thing to be told.
2. **Resolutions get shorter as they get better.** Compression is the legible sign of
   thinking working.

**This stack is the drift feature.** Better than word-embedding semantic drift: your own
stated positions on one question over time, in your own words, with what broke each one
recorded in between. Works from the third entry rather than the three-hundredth. No
clustering, no Procrustes alignment, no years of corpus needed.

**Resolution must never feel like an achievement.** No checkmark, no green. If closing a
thread is satisfying, threads get closed for the satisfaction, and premature closure is the
exact failure this exists to prevent. In ring language: the outermost ring goes solid, and
breaks again on reopening.

---

## 7. Data model and retrieval

```
entries   id, audio_path, transcript, created_at, x, y,
          parent_edge, type, resolved, resolution_text
vectors   entry_id, topic_vec, move_vec
edges     id, entry_a, entry_b, relation, question, status, created_at
spans     entry_id, start, end, attributed  -- quoted vs own
```

`parent_edge` is what turns one-offs into threads. A response is an entry with it set — no
separate type, no separate table.

### 7.1 Two vectors

**`topic_vec`** — embedded from the generated one-line summary, not the raw transcript. A
six-minute ramble embeds mushy; digressions dilute it. The summary is cleaner for
similarity even though it's never displayed over the user's words.

**`move_vec`** — embedded from an LLM-extracted abstraction of what the entry *does*:
"denies X is exempt from a general pattern", "treats a process's non-termination as
evidence of real structure".

This is the differentiator and it came out of real data. Across the user's own notes, an
argument about microbes and divine hierarchy, one about AI-generated art, and one about
their own writing process shared essentially no vocabulary — but made the same move three
times (refusing human exceptionalism). Cosine similarity on topic would never have
surfaced them together.

Confirmed again on a stranger's corpus: two notes written days apart, "the US is the
frontrunner of digital colonialism" and "the US uses economic sanctions to exert power over
other countries." Different clusters, different vocabulary, same underlying claim.

### 7.2 Background pass

**Excludes the last ~2 weeks.** Every proposed pairing is therefore cross-time. That one
line is most of the product.

Pipeline: new entry → transcribe → summary → both embeddings → store. Later: nearest
neighbours on both vectors (different thresholds), one cheap LLM call per candidate pair
asking *what* the relation is, emit edge only if it can name one.

### 7.3 Attributed spans

**Exclude quoted material from the move vector.** Speech marks attribution reliably —
"so this guy was saying", "I read somewhere that", "there's this idea that". Tag spans as
attributed or own. Topic vector may use everything; move vector uses only the user's own
spans.

Without this, a note where the user quotes three people gets connected on *their* thinking.
On the real commonplace wiki examined during design, more than half the content of a mature
note was block quotes from sources.

Given the commonplace framing, consider storing excerpt and gloss as separate fields from
the start rather than separating post-hoc.

### 7.4 Source material has no marker yet — open

The corpus examined had book notes, article notes, and the user's own writing about a book,
all rendering identically. Structurally the *bridges between territories tend to be source
notes rather than the user's own claims* — which is worth seeing at a glance, since a
corpus where all connective tissue is other people's is a different thing from one where
you're making the links yourself. **Needs a marker. Not designed.**

---

## 8. Visual design

**Surface:** near-black `#0B0D0E`. Space, not desk/paper/whiteboard. Two reasons: a sparse
corpus and a dense one both look right on it (white makes emptiness visible, and twenty
entries read as failure), and it doesn't imply productive labour — which matters when some
entries are personal. Test: what does it feel like to open at 2am after recording something
painful. Whiteboard says get to work. Void says it's just there.

**Visual quiet, not cognitive restraint.** These are different claims and an earlier draft
conflated them. Fewer competing elements means less to read past, and the thing to attend
to is the user's own words. But the research points the *other* way on effort: the CSCW
2025 study (Chen et al.) found the most automated note-taking assistance produced the worst
learning while being the most preferred, with users modifying only ~8% of auto-generated
content vs ~40% in the intermediate condition. So: **automate the mechanical** (placement,
transcription, retrieval, IDs, titles) and **preserve the effortful** (judging a connection,
answering the question, stating a resolution).

**Palette**

```
surface            #0B0D0E
blob fill          #1C2224
blob fill (felt)   #2B3538 + gaussian blur, no stroke
stroke ordinary    #5C6A6D
stroke weighty     #7C8B8E
stroke hub         #8DA1A4
edges              #2A3335
thread edges       #5A6B6E
titles             #9DACAF
edge labels        #6E7C7F
dimmed / isolated  #6E7C7F on #171D1F
rings              #252E30 / #2E3739 / #3E4B4E (outer→inner)
THE one colour     #D85A30   (unanswered question dot only)
```

**Type:** system sans throughout, small — 11px canvas titles, 10.5px edge labels, 12px
metadata. **One exception:** the question, ~15px serif. It is the only voice in the app
that isn't the user's and should visibly not be theirs.

**Motion:** nothing animates except live audio. Equalizer bars while recording; the
currently-playing blob. If idle blobs pulse ambiently the canvas becomes an aquarium and
within a week you stop seeing any of it. **Motion means exactly one thing: audio is running
now.** This also answers "which one is playing" with no extra UI, and the same visual
language carries to the tray indicator.

---

## 9. Stack

### 9.0 Required technology — fixed, not negotiable

| layer | requirement |
|---|---|
| desktop shell | **Tauri + Rust** |
| frontend | **Next.js with static export** |
| transcription | **transcribe.cpp**, via its Rust SDK / C bindings — local, always |
| LLM | **llama-server** with local GGUF model management |

Everything below is implementation detail *within* those four constraints. The LLM sits
behind a provider interface (§9.4) so a remote model can be used for the demo, but the
llama-server path is a requirement and must be a working implementation rather than a stub.
Audio and embeddings are local unconditionally.

**Rust:** global hotkey (tauri global-shortcut), `cpal` audio capture, transcribe.cpp
bindings, `fastembed-rs` (ONNX) embeddings, SQLite, brute-force cosine. A few thousand
384-dim vectors is ~15MB and sub-millisecond to scan. **You will never need a vector
database.**

### 9.1 Next.js static export inside Tauri

Workable if treated as a static SPA generator rather than a framework.

```
next.config.js   output: 'export', images: { unoptimized: true }, trailingSlash: true
tauri.conf.json  frontendDist: "../out", devUrl: "http://localhost:3000",
                 beforeDevCommand / beforeBuildCommand → npm scripts
```

Lost: API routes, server components doing server work, middleware, SSR, ISR. Everything is
`"use client"`. Don't use App Router routing for the canvas — it's one page with panels.
**Tauri commands are the API layer**, called via `invoke()`; anything that would have been
a route handler goes in Rust, which is where the real work belongs anyway.

Snags: `next dev` port must match `devUrl` exactly; on Windows bind the dev server to a
host Tauri can reach.

### 9.2 Rendering

**2D canvas, not 3D.** 3D was considered (blobs in space, size by duration) and cut. Not
because it's hard — react-three-fiber is a few extra days — but because depth costs camera
controls, occlusion handling, and fiddlier hit-testing while buying nothing that size and
position don't already give. On a flat plane size reads as size, with no perspective
ambiguity. Konva, PixiJS, or plain canvas 2D with own hit testing (a few thousand circles
is not much code and no dependency).

### 9.3 Memory budget

```
idle in tray (no webview)      ~15–30MB
canvas open                    ~40–80MB   (OS webview, not bundled Chromium)
transcribe.cpp model           ~75 / 140 / 470MB (tiny/base/small) — load per use
embedding model (MiniLM)       ~90MB      — load per use
llama-server GGUF (Q4)         ~1GB (1.5B) / ~2GB (3B) / ~4.5GB (7B) — see §9.4
```

Transcription and embeddings are bursty — a few seconds per recording — so load and unload
them per use rather than holding them. Destroy the webview on window close rather than
hiding it.

**llama-server is the exception and the whole budget question.** If it stays warm it
dominates everything else on this list by an order of magnitude; if it spawns cold, peak is
the larger of transcription and the model rather than the sum, but the post-recording
question gets slow. §9.4 has the fork.

Rough targets: **~25MB idle** with a cold model, **~120MB** with the canvas open and no
inference running, and a spike to whatever the resident GGUF costs during generation.

### 9.4 LLM — provider abstraction, local and remote

**Audio never leaves the machine.** Transcription (transcribe.cpp) and embeddings run local,
unconditionally. No toggle, no provider interface, not configurable. That is the firm line.

The reasoning LLM is behind a provider interface with two implementations:

```rust
trait LlmProvider {
    fn generate(&self, prompt: &str, opts: GenOpts) -> Result<String>;
    fn name(&self) -> &str;          // shown in UI — user always knows who answered
}
```

- **`LlamaServerProvider`** — spawns and manages llama-server, handles GGUF download,
  verification, selection and swap. **This is the default and the thing you demo.**
  llama-server with local GGUF management is named explicitly in the brief, so it must be a
  working, first-class path — not a stub, not a fallback.
- **`RemoteProvider`** — HTTP to a hosted model. An *addition*, offered as an option. The
  brief doesn't ask for it, so it's a bonus; it must never be the reason the product looks
  good.

**Demo posture:** local, with a model size chosen by the §13 sweep — the smallest GGUF whose
questions you'd actually want to answer. Leading a demo with remote against an explicitly
local requirement reads as sidestepping it, however good the questions are.

If no local size clears the bar, **say so in the README as a finding** rather than routing
around it. "Questions at 3B were generic; at 7B they were usable; here's the tradeoff and
here's why we ship the provider abstraction" is stronger evidence of technical judgement
than a demo quietly powered by an API.

Build the abstraction early with both implementations. Left until last, the local path ends
up a stub — and that's the version that fails an offline check.

**Privacy — the tension returns.** With remote available, some entries would leave the
machine, and the uncomfortable fact resurfaces: the entries most worth probing are exactly
the ones you'd least want sent anywhere. So:

- Per-entry **local-only** flag, set by the user at capture. Never inferred by a classifier.
- Global default is the user's choice in settings.
- An entry marked local-only **never** reaches the remote provider, including in background
  passes and including as context for a *different* entry's question.
- The UI states which provider produced any given question or analysis.

**Model residency — still a real fork for the local path.**

```
Q4 GGUF footprint       1.5B ≈ 1GB    3B ≈ 2GB    7B ≈ 4.5GB
cold start (load)       several seconds from disk
```

llama-server is a persistent process, and two options push the UX differently:

- **Warm** — stays resident. Post-recording question in ~2s. Costs 1–4.5GB held by a tray
  app.
- **Cold / spawn-on-demand** — RAM returns between uses. Costs 10s+ before a question
  appears.

Not all AI work is latency-sensitive. The background pass (edge generation, move-vector
extraction, summaries) batches and runs cold on idle — nobody is waiting. **Only the
post-recording question is latency-sensitive**, because it's meant to land while you're
still in the thought. A reasonable split is a small warm model for that one call, larger
spawned cold for background analysis.

If warm residency proves unacceptable on the local path, the fallback is already designed:
drop the immediate post-recording question for local users, keep the panel, and let the
question surface when the entry is next opened (§4, §6.1). Capture is unchanged and nothing
else breaks. Remote users keep the immediate question.

**Model management is a first-class feature**, not plumbing: download, verify, select, swap,
and show which model is answering. Expect to change models often while tuning §14's prompt,
and expect question quality to move noticeably when you do.

### 9.5 Transcription

**Errors are a correctness threat, not just an annoyance.** If transcription mishears the
word a claim turns on, anything built on it is an artifact. Store audio alongside text,
keep per-word confidence, and exclude low-confidence tokens from similarity scoring.
Transcription runs local via transcribe.cpp regardless — audio is the most sensitive
material here.

---

## 10. Evidence base

Short version of what the research supports and what it doesn't.

**Supports the design:**

- **Shipman & Marshall, "Formality Considered Harmful" (1999).** Users resist making
  structure explicit — cognitive overhead, tacit knowledge, premature commitment. Their own
  proposed remedy is *system-assisted incremental formalization*: have the system suggest
  structure rather than demand it. That is this app.
- **Chen et al., CSCW 2025 (DOI 10.1145/3757632).** N=30, three levels of AI note-taking
  assistance. Intermediate (AI supplies candidate blocks, user curates) scored highest on
  learning (M=13.88); Automated (AI writes finished notes) scored lowest (M=10.28) — while
  being the *most preferred*. Users modified ~8% of automated content vs ~40% of
  intermediate. Direct evidence for candidate-suggestion over auto-application.
- **Anderson, Shah & Kreminski, C&C 2024 (arXiv:2402.01536).** 36 participants: ChatGPT
  users produced *less semantically distinct* ideas than users of an Oblique Strategies
  deck, generated more detailed ideas, and *felt less responsible for them*. A naive AI
  integration makes your writing blander and less yours.
- **Bergman & Whittaker, *The Science of Managing Our Digital Stuff* (MIT, 2016).** People
  prefer navigation to search; tagging and rich classification are widely abandoned in
  personal collections. Justifies auto-placement and auto-linking.
- **Generation effect / desirable difficulties** (Slamecka & Graf 1978; Bjork & Bjork 2011;
  McCurdy et al. 2020 meta-analysis, 310 experiments). The effortful act is the mechanism.
  Caveat: only difficulties that engage the *target* process help — arbitrary friction does
  nothing. Manual ID-typing is undesirable difficulty; judging a connection is desirable.
- **Kirsh, "The Intelligent Use of Space" (1995); Kirsh & Maglio (1994).** Spatial
  arrangement is cognition, not decoration — but only when positions are stable and chosen.

**Does not support:**

- **There is no controlled evidence that Zettelkasten-style linking improves thinking or
  output.** The reputation rests on one prolific man's testimony. Don't claim otherwise.
- Luhmann's actual system (Schmidt's archive work): ~90,000 cards across *two* structurally
  different boxes, a deliberately *sparse* keyword index (1–4 locations per term), and
  roughly **one link per card** — not a dense mesh. His headline finding: *"the money is in
  the hubs"*, i.e. a few high-degree structure notes, not a flat graph.
- Word-level semantic drift on a personal corpus is **unproven**. Every method (Giulianelli
  2020, Gonen 2020, Montariol 2021) was validated on large corpora; reliability at
  journal scale is uncertain and likely noisy. The resolution-stack approach (§6.3) is
  strictly better here and should be the drift story.

**Useful prior art:**

- **Joel Chan** prefixes note titles by argumentative role — QUE / CLM / EVD / PTN. That's
  the move vector, done by hand, by someone who found it worth the effort.
- **Sensecape** (Suh et al., UIST 2023, arXiv:2305.11483) — closest published system;
  multilevel canvas over LLM output. Users structured knowledge more hierarchically
  (M=4.3 vs 2.6) and revisited prior information far more (M=12.8 vs 0.7).
- **nodepad.space** (open source, mskayyali/nodepad) — spatial canvas, AI classifies notes
  into 14 types and surfaces what you *haven't* said. Provocateur pattern already shipped.
  No time dimension.
- **Ink & Switch Potluck** — "gradual enrichment from docs to apps"; incremental
  formalization for the LLM era.

**Real corpus observations** (Neil Mather, commonplace.doubleloop.net, ~7,157 pages):

- He has a section literally headed **"Unzettled"** holding open questions — his own
  unfinished marker, by name. The one under digital colonialism questions his own *framing*,
  not his conclusions.
- **Return depth is five years deep.** One hub planted July 2021, last tended July 2026,
  with backlinks from dated journal entries in 2022, 2024, 2025, and twice in July 2026.
  Rings aren't a nice idea; that's what a live question looks like over time.
- His claim notes have **full-sentence titles** ("Digital colonialism is a form of
  neocolonialism"), concept notes have single-term titles and act as hubs, and source notes
  are books/articles. Three node types emerging without design.
- **No felt entries anywhere.** Nobody publishes those. Published gardens are a biased
  sample and can't validate the emotional half of this design.

---

## 11. Rejected, and why

Recorded so they don't get re-proposed.

| rejected | why |
|---|---|
| **Obsidian plugin** | Evaluation wants a build, not an extension; a reviewer won't install Obsidian to see it. Obsidian gives ~90% of substrate and 0% of the idea (nothing temporal; every AI plugin is chat-over-vault). |
| **3D canvas** | Camera controls, occlusion, fiddly hit-testing; size + position already carry it. Every Obsidian 3D-graph plugin is force-directed spectacle. |
| **Unplaced tray requiring manual drag** | Overhead kills daily use; PIM research says filing gets abandoned. |
| **Force-directed layout** | Re-solves on insert, nothing stays where you left it, spatial memory dies. |
| **Answers as separate blobs** | Node explosion; fragments meaningless alone. → rings. |
| **Emotion tags / colour-coding feelings** | Flattens exactly what's worth keeping. "Warm and weird and nice" is not `#grief`. A canvas where your grief is blue is grim. |
| **Auto-routing probes by type** | A misclassified felt entry getting Feynman-probed is unrecoverable. Classification suppresses, never selects. |
| **Contention as a button you press** | The user would never press it — same reason the Keep notes were never reopened. Trigger is post-recording, while still in it. |
| **"Argue with this" affordance** | A chat button in disguise. The question *is* the response. |
| **Summary above the transcript** | You read the tidy version and skip your own words. |
| **AI rewriting / "refining" notes** | Demonstrated failure: a GPT rewrite of the user's own paragraph replaced "warm and weird and nice" with "nice in a way that felt almost out of place", and turned a rule they were telling themselves into a settled conclusion. Same failure as the echo chamber, different register — it agrees with your *composure* instead of your argument. |
| **Chat panel** | The thing this exists to replace. |
| **Word-embedding semantic drift as the feature** | Unproven at personal-corpus scale; the resolution stack does it better and works immediately. |
| **"Second brain" positioning** | See §12. |
| **Verdicts of any kind** | Something to argue with instead of something to think about. |

---

## 12. Positioning

**Not a second brain.** Mechanically it shares the machinery — capture, storage, retrieval,
connection. But *second brain* means storage and retrieval, and the value proposition is
accumulation, which is the exact failure mode here (the collector's fallacy). A tool
measured on how much it holds is a different tool from one measured on how often you came
back.

The term also sets the wrong expectation on arrival. Someone hearing "second brain" imports
a reference library, sees grey blobs with no edges, no rings, no questions, and concludes
it's broken. Someone told "it argues with you about what you've said" records three things
and immediately gets it. **The category tells people what to bring.**

It needs a corpus where you take positions and change them. That isn't unique — anyone who
argues with themselves in writing has it — but a vault of book summaries and technical
reference genuinely gets nothing, and no tuning fixes that. Every edge would read "related."

PKM-adjacent is fine as a shelf. Don't let it be the pitch.

---

## 13. Build order

1. **Capture + canvas.** Hotkey, transcription, blobs, frozen placement, titles. Success
   criterion: the user opens it daily for a month without friction.
2. **Move-vector extraction + edge generation.** The differentiator; everything downstream
   needs it. Contradiction retrieval (mode G) first — it's the strongest and the least
   dependent on prompt quality.
3. **The question**, then threads and resolution.
4. **Drift** falls out of the resolution stack. Nothing extra to build.

### Test the question before building any of it

Record five rambles this week. Transcribe them however. Hand each transcript to a model with
the prompt you'd actually use. Read the five questions.

- Two you'd genuinely want to answer → the mechanic works.
- All generic → you learned it for an evening instead of a sprint.

Run the local GGUF sweep **first** — two or three sizes under llama-server — since that's
the required path and its result decides both the demo model and §9.4's warm-vs-cold fork.
Run a remote model afterwards as a control: if remote produces good questions and local
doesn't, the mechanic works and the constraint is model size, which is a finding worth
reporting. If remote is also generic, the prompt is the problem.

**The fallback is intact either way:** drop the post-recording question, keep the capture
panel, and let contention live only at connections — where the model has two entries and
something concrete to point at, rather than having to invent a question from one ramble.
The capture flow is unchanged.

### Seeded demo corpus — a deliverable, not a shortcut

Every mechanic that distinguishes this product needs a corpus with time depth: cross-time
connection, contradiction, return rings, resolution stacks. A reviewer opening an empty app
sees grey blobs and nothing else, and concludes it doesn't do much.

Ship a seeded vault of 20–30 entries with backdated timestamps spanning roughly a year:

- several entries circling one preoccupation in shifting vocabulary (drives *same move*)
- at least one pair that flatly contradicts (drives *contradicts* and the resolution stack)
- one thread with two resolutions, the second breaking the first (drives rings)
- one intent note and a later entry showing it never happened (drives *returns to*)
- one or two entries containing action items (drives §1.2)
- two or three entries connected to nothing, sitting in empty space — the honest case

Writing this is also a test of the design: if you can't construct a corpus where the edges
are obviously right, the thresholds or the move vector need work before more UI does.

Real audio for a handful, synthetic transcripts for the rest — the fingerprints only need
amplitude data, which can be generated.

### Failure thresholds

- Can't use the Stage 1 tool daily → the problem is capture friction, not AI. Fix that
  before adding anything.
- Edge acceptance >85–90% without edits → passive-acceptance regime (the Chen et al. 8%
  signature). Raise the threshold or reduce suggestion volume.
- Finding yourself explaining yourself to it → stance is wrong (§3.5).
- Competing on canvas features → you've drifted into the crowded market. Re-anchor.

---

## 14. Still open

- **The question prompt itself.** Weeks of iteration; can't be designed in the abstract.
  Needs the surrounding transcript and any linked entries, not just the selected sentence,
  or it goes generic and gets ignored.
- **Source-material marker** (§7.4).
- **Distinguishing "challenges the answer" from "challenges the question"** (§6.3). Worth
  a lot if achievable.
- **Within-entry reversal.** Speech contains real-time turnarounds — "actually no, hang
  on" — and the moment someone reverses out loud is usually the most alive part of a
  recording. Currently undetected.
- **Screens not drawn:** empty state (matters unusually here since the design assumes a
  corpus), sparse state (three entries, no edges — more awkward than empty), the
  transcription gap, low-confidence transcript, dense canvas proving LOD, search.
- **Search.** No way to find anything by remembered phrase.
- **Deadline and deliverable format** for the evaluation — unanswered, and it determines
  how much of the above gets built vs. written up as direction.

---

## 15. Implementation status — 3 September 2026

Written after the mockup build. Everything above is the design as specified; this section
records what exists and where the build departs from it. **Where the two disagree, this
section is the current state.** §5.2 in particular is now historical.

The repo is a UI mockup running on the real stack: the Next.js frontend is complete and
exercised against a fixture backend, and `lib/bridge/` is the only seam.

### Built, and verified running

- Capture panel states, entry view with proposed-edge cards, resolution stack, action items
  and the global task list, empty state, sparse notice.
- **Onboarding**, rebuilt: the tagline gets real size as the app's one moment to introduce
  itself, the three settings rows sit in a suspended pill, and Start echoes the action pill
  in the main app. Flow is unchanged and deliberate — onboarding, then the *empty* canvas,
  with the sample-corpus link kept because this build is a mockup.
- **Search** over transcripts with match snippets — §14 listed this as missing.
- Canvas with LOD and markers, on the frozen placement algorithm.
- Type registry with user-defined types (§3.6): render-slot binding, mark validation, and
  the safety gate.
- Light and dark themes; export / upload / delete; floating chrome.

### Not built

- **The Rust command layer.** `TauriBridge` is written in full and names commands that do
  not exist yet. That file remains the specification for the command surface.
- transcribe.cpp integration, llama-server provider, real embeddings.
- **Manual edge drawing** — §5.4 says it must exist. Still absent.
- The source-material marker (§7.4).
- `src-tauri/` does not compile on this machine: VS Build Tools 2026 is installed without
  the Windows SDK, so `kernel32.lib` is missing and no MSVC target can link. Nothing in
  `src-tauri/` needs to change; the SDK component does.

### Superseded by the build

**§5.2 Blobs — replaced by a typographic canvas.** There are no blobs. The title *is* the
node, sized by duration on the same square-root curve the radius used. Type is carried by
the letterform — serif 600 / italic / light-tracked / mono — plus a leading mark on a rail,
rather than by edge treatment. The audio signature sits beneath each title. Pixi now draws
only edges and the hover ring; everything else is DOM. The crisp / irregular / soft
vocabulary survives only in `edgeTreatmentFor`, which nothing calls.

**§5.4 Edges — labels are hover-only.** "Horizontal label at the midpoint" held until the
corpus carried twenty edges, at which point the map read as a net rather than a map. Labels
now appear only for the hovered entry's own edges. The rest of §5.4 stands unchanged: the
seven nameable relations, the draw-nothing rule, thread edges heavier than proposed ones.

**§5.1 Placement — same algorithm, box-shaped.** Still frozen, still one-shot per entry in
chronological order, nothing already placed ever moves. Three changes: collision is the
title's rectangle rather than a circle drawn around it (a circle reserves the empty corners
of wide, short text, which made the map look scattered while still colliding horizontally);
the overflow spiral starts pointing away from the field centroid instead of at a random
phase; and isolated entries take a ring stepped by golden angle rather than a random
compass bearing, which was blowing the bounding box out to one side.

**§3.6 — user types carry their own mark.** The stored/rendered split is unchanged and
rendered slots remain 3 + inert. Added: a user-defined type supplies a *character* as its
mark, validated against four failures — colour emoji, a glyph the font lacks (which would
render as a box identical to the inert mark), too much ink, and the disc shape reserved for
the unanswered question. Built-in marks stay drawn paths so they are exact at 11px and
independent of font availability. Measured on the app's real mono stack: built-in marks
occupy 0.10–0.16 of the mark box; the cap for a pasted character is 0.30.

Also added, because §3.6 rule 1 had no mechanism: an explicit `autoApproved` flag. Tier
alone could not express "defaults to invoked-only, user may opt into automatic" without
making opt-in permanently unreachable.

**§8 Palette — two themes.** The near-black surface is now the dark theme, still the
default on a dark OS. A light theme was added from the lexicon specimen's palette:
`#F1F3F3` ground, `#14181A` ink, `#79858A` mid, `#DFE4E5` rules, `#1D4ED8` accent. This
weakens §8's void-not-whiteboard argument, which was a real argument — the light theme is a
preference offered, not a refutation of it.

**§8 Motion — one deliberate violation.** Side panels slide out over 190ms. §8 says motion
means exactly one thing, audio running now. This was requested directly and knowingly, and
it honours `prefers-reduced-motion`.

**§9.1 chrome — floating pills.** One page with panels as client state is unchanged. The
chrome is no longer edge-docked: a search pill below the top edge, status and an action
pill above the bottom edge, and the entry / settings / tasks sheets and the legend as
suspended panels inset from the edges.

**Bridge gained `importCorpus(data, mode)`**, merge or replace, so restoring an export is
part of the Rust command surface rather than a client-side shortcut.

### Seeded corpus

§13 calls the demo corpus a deliverable. It is now 20 entries on growth, limits and
technology — decoupling, the transition build phase, data centre water, the WMO figure,
targets as ritual, ownership under the AI energy argument — deliberately littered with
unconnected material (a heron, a failed loaf, a publishing bug) so the isolated treatment
has something to act on. Five entries connect to nothing and render recessed.

Earlier corpora are kept: `fixtures/corpus.21.json.bak` (the original) and
`fixtures/corpus.degrowth-core.json.bak` (the same subject before littering).


### Accepted for the mockup

Known and deliberately not fixed while this is a UI mockup rather than a running app:

- **Onboarding shows on every launch** in practice, because settings live in localStorage
  and get cleared during testing. Not a defect to chase yet.
- **The theme choice is not persisted.** It lives in the zustand ui slice and resets to
  `system` on reload, unlike settings, hotkeys and canvas positions which do persist.
- **Onboarding and the empty state share a sentence** verbatim. They should do different
  jobs — what the app is, versus what to do next — but the duplication is cosmetic.
- **Search dims titles but not edges.** Lines stay full strength over faded text. Never
  specified either way.

### Fixed after the section above was written

- **Search had stopped dimming the canvas entirely.** The dim lived inside `drawBlobs`,
  deleted in the lexicon conversion, so `setMatched` was still called and nothing consumed
  it. Node opacity now composes letterform x isolated x search through a CSS variable, so a
  React re-render cannot clobber the imperative dim.

### Still open

Everything in §14 stands except **search**, the **empty state** and the **sparse state**,
which are built. Added since:

- **Edge labels dodge titles but not each other.** Two revealed labels can still overlap.
- **The relation vocabulary may be too thin.** Seven fixed words carry less weight than the
  entries they join. A spec-level question, not a bug.
- **Similarity is mocked** from `storedType` plus an id hash, so cross-topic pulls are much
  weaker than real embeddings would give — a `contradicts` edge does not pull at all, only
  topic similarity does. Layout quality will change once embeddings are real, and some of
  what currently looks like a placement weakness is this.
- **Tier demotion is unverified in the running app.** The `safe → heavy` clamp for
  non-approved user types is typechecked and simple, but has not been watched fire.
- **The legend reserves 172px permanently**, whether or not the corpus needs it.

---

## 16. Accessibility pass — 3 September 2026

Run against the `ui-ux-pro-max` guidance. The style search matched
`minimalism-and-swiss-style`, whose own keyword list includes *high contrast* — the build had
the minimalism and had inverted the contrast. No change to the design language followed from
this; every failure was in the chrome, and the canvas was left alone.

### Tokens

- **`--meta` / `--edge-label`** lifted. Dark `#6e7c7f → #7d8b8e` (3.94 → 4.83 against
  `--dimmed-bg`, where nearly all of this text actually sits). Light `#79858a → #5f6a6e`
  (3.14 → 4.60). `--dimmed-fg` shares the old dark value but is a canvas semantic — "connects
  to nothing" — and deliberately did not move.
- **`--edge-control` is new.** `--edge` is 1.51:1 on the void, which is fine for a decorative
  hairline and not for a border that is the only thing identifying a control. Six borders moved
  to it — the search field, the typed composer, the probe buttons, the resolution field, and
  the type editor's select and on-chip. Everything else keeps `--edge`.
- **`--size-micro: 11px` is the type floor.** 29 declarations sat below 12px, bottoming out at
  6.5px in the legend glosses. All of them now read the token, so the floor is a constraint
  rather than a coincidence. `--size-edge-label` went 10.5 → 11px with them.

### Target size

Nineteen controls were the same idiom — `background: none; border: none; padding: 0` — so the
hit area was exactly the glyphs, around 12x14px against WCAG 2.2 AA's 24x24. Padding plus a
negative margin would have eaten the flex gaps they sit in, so each grew a centred
pseudo-element instead:

```css
width: max(calc(100% + 8px), 24px);
height: max(calc(100% + 10px), 24px);
```

Layout-neutral by construction. Measured on the entry view's `esc` button: layout box 19x13,
hit area 27x24.

### Manual linking now has three doors

The hover stub was the only route, which is the guidance's named High-severity anti-pattern and
left touch and keyboard with no path at all. The stub stays and is joined by:

- **`connect to...`** in the analysis block, beside the proposed connections.
- **`C`** on an open entry (suppressed while a field has focus).

Both open `components/canvas/ConnectPicker.tsx` — a filterable list of entries, filtered on
title and transcript, excluding the entry itself, its layers, and anything already linked to it.
Arrow keys move, enter commits, escape leaves. All three doors end at the same
`RelationPicker`, so this is one concept with three entrances rather than three features.

New state: `connectSource: string | null` in the ui slice. `setPendingLink` clears it, so the
two stages cannot both be live.

### Fixed in passing

- **`.resolveOpen` and `.analysisToggle` were colliding.** `.sheet` is a block container, so
  both buttons flowed onto one line and rendered as `landed on something?analysis`. The
  `align-self: flex-start` on the first was a no-op there. Both are `display: block` now. This
  predates the pass.
- **Legend glosses moved off mono.** At the 11px floor they wrapped three ways in the column,
  and mono is reserved for the machine voice (§8) — a gloss is editorial prose. `--legend-w`
  went 148 → 172px to match.

### Verified

Typecheck and static export clean. The full keyboard path was driven end to end: `C` on an open
entry opens the picker with focus in the filter, arrows move the cursor, enter opens the
relation picker with the correct pair, and a relation commits the edge. The drag path was
re-driven with a stepwise pointer trail rather than a teleport, which is what had hidden the
original handle bug.

### Probes became one door — 3 September 2026

The analysis block offered four named techniques — `steelman it`, `find the edge`, `what would
break`, `ask why four times`. That asks the user to pick the thinking tool, which is the model's
job and the wrong question to put in front of someone who has just finished talking. Replaced by
a single **ask it something** with the hint *it picks what this entry needs*.

Nothing about §3.6 changed underneath. `invokedProbes()` still gates on the entry's stored type,
so felt and inert entries reach nothing and the button does not render for them; the choice is
made from the eligible set rather than from all of them.

- **`askQuestion(entryId)` is new on the bridge** and is the only path the UI takes.
  `runProbe(entryId, probeId)` stays as the primitive it picks from — a named probe is still
  worth being able to replay or evaluate, it just isn't a control any more.
- Mock picks the first eligible probe. Real inference would weigh the transcript instead; the
  gating is the part that carries over.
- **`connect to…` now sits below `ask it something`**, not above it.

### Still open after this pass

- **The canvas itself still has no keyboard navigation.** You can link from an open entry
  without a pointer, but you cannot *open* one without a pointer, so the new route is only as
  reachable as the entry it starts from. The host div has no `tabIndex` and no `onKeyDown`.
- **Contrast was only checked against the two theme grounds**, not against `--blob-fill` or the
  ring colours, which some canvas text crosses at low zoom.
- **`--edge` still fails 3:1 everywhere it outlines a panel.** That is defensible while those
  panels have their own background, but it has not been audited case by case.

---

## 17. Taxonomy recut and the answer model — 4 September 2026

Where this section and anything above it disagree, this one is current. Two
entries below **reverse recorded rejections**; they are marked as reversals so
they don't read later as things nobody noticed.

### The four types were mis-cut, not too few

`claim / rant / felt / inert` were divided on four different bases — rhetorical
form, epistemic state, affective register, and a null class — so they were never
mutually exclusive. A heated unresolved argument about your job is the first
three at once and the model had to pick arbitrarily. Worse, picking `felt` to
stay safe also removed the entry from retrieval, so protecting an entry cost you
what it meant.

Three orthogonal facets replace them:

| facet | values | drives |
|---|---|---|
| **role** | `position` · `evidence` · `note` | letterform, mark, retrieval |
| **register** | `live` · `neutral` | the automatic question only |
| **provenance** | `own` · `attributed`, **per span** | what a question may anchor to |

`position` absorbs claim and rant — that difference was always
resolved-vs-unresolved, which `resolved` already carries deliberately, and it was
the hardest call for the model while buying only a font weight. `felt` became
register. `inert` became `note`. The italic that meant `rant` retires; the rings
say unresolved better than a slope does.

**Register is whole-entry and must never become span-level.** Habernal & Gurevych
(2017) ran this exact two-axis scheme over the same corpus: argument role reached
aU 0.48, the affect layer reached 0.30 and was dropped from their released model.
The failure was localisation, not affect. Provenance is the span-level facet;
register is not.

**Do not gate on the model's own confidence.** Seven 3-9B instruct models tested
in 2026 all scored invalid on confidence validity, ceilinged at 91.7% regardless
of accuracy — squarely the range §9.4 offers. Tune sensitivity by prompt instead;
a crisis-detection study drove false negatives 87% → 0% that way, precision
falling 91% → 63%, which is the affordable direction.

**Nobody has measured machine-assigned facets against a flat label, in any
domain.** §13's self-agreement test is therefore not optional — it is the only
way to know whether `register` is callable on this corpus.

### Marks, and what §5.2/§5.3 now say

Three letterforms, not four: serif 600 upright, serif 400 upright, mono. Three
drawn marks: an assertion stands up, a fact lies flat, a note is a hollow square.
`live` composes on top as a treatment — softer, tracked out, dimmed — so a live
position still renders as a position. The canvas carries role, register and
provenance on three independent channels, which is what §5.3's budget actually
described.

### Gate changes

- **§3.2 was being applied to both paths.** Suppression is scoped to the
  automatic question in the spec, but `mayProbe` ran it on the invoked path too,
  so a `live` entry reached nothing at all. Now split: `mayProbeAutomatically`
  (all three facets, fails closed) and `mayProbeOnRequest`. Register does not
  gate the invoked path — §3.2 already said the risk there is the user's.
- **§3.1 F was mis-gated.** Feynman required `position` *and* an attributed span.
  A note recording that something clicked classifies as `evidence`, so the one
  move that makes the user do the thinking was the least reachable in the app.
  Feynman now needs only "not a note" — it takes no stance and cannot misfire the
  way a steelman can.
- **§3.6 rule 1 is gone.** User types fire on their own like built-ins. The
  suppressions under it are the ones with teeth: role, provenance and register
  are not overridable, so a user's tier can only narrow what those already allow.
- **§7.3 now reaches the gate.** `Span.attributed` fed only the move vector while
  the probe picker anchored anywhere, so asking about a note on a book steelmanned
  the book's own sentence back at you — the case §3.3 forbids. Anchors now exclude
  attributed spans.

### Questions accumulate — §3.4 upheld, and given teeth

`questions` is `Map<string, Question[]>`. Nothing is replaced. §3.4 bans a
regenerate button and that holds, but accumulation makes rerolling *structurally*
impossible rather than forbidden by policy: the question that stung is still on
the entry. A bad question is **dismissed** instead — struck through, kept, no
longer open. Those dismissals are also the negative examples a local prompt bank
needs to become the user's rather than Qwen's.

**Invocation moved onto a text selection**, which is what §3.2 always said
("user selects a passage"). §11 rejected contention-as-a-button twice and the
build had reintroduced one; it is gone. Selecting also makes the provenance check
exact rather than inferred.

### REVERSAL — answers are notes, not layers

§11 rejected "answers as separate blobs" for node explosion and fragments being
meaningless alone. §6.2 had answers thicken the parent's rings instead. That is
reversed: an answer lands on the canvas, carries an accepted edge back to what it
answers, and is eligible for its own question.

Both risks §11 named are real and **neither is addressed**. Density has not been
tested. The edge and the lineage are what keep a fragment readable. If the canvas
becomes a hairball at scale, this is the first thing to look at.

`extends` is the relation used, as the closest of the seven. Nothing in the closed
vocabulary means *answers*; if answering becomes common it probably earns an
eighth word.

**Any open entry now takes your voice.** The hotkey previously required an
unanswered question, so an entry you had already answered could not be spoken
into and the recording became an unrelated note. Returning is the mechanic the
rings exist to count.

### Onboarding — four beats

Eleven of twelve font-size declarations were `--size-micro`. Mono carried the
app's own words, inverting §8's rule at the moment the app introduces itself. A
reviewer's first word for it was "AI slop".

1. **Wordmark and models.** Serif at display size, plain labels — `Transcription`
   and `Reasoning`. Downloads start here.
2. **What a note can be.** The three kinds, each as *what you said* → *what it
   says back*, in the two voices the entry panel uses.
3. **How notes find each other.** One dated pair, the relation, the question.
4. **Setup.** Hotkey, microphone, model progress.

Screens 2 and 3 get real reading sizes; §8's visual quiet governs the canvas, and
those screens are not one. The relation vocabulary, the draw-nothing rule and the
open-questions node were all cut — each was the spec talking rather than the
screen showing.

### Settings

Every row and field in the type editor is glossed. The gloss under each built-in
is the same string the classifier matches against, so what the user reads is what
decides the type. Tier renders as behaviour — *asks on its own* / *only when you
ask* / *never asks* — because tier alone stopped describing behaviour once
Feynman was ungated: `evidence` and `note` are both silent and behave differently.

### Corpus

Relabelled to 14 `position`, 4 `evidence`, 2 `note`; 8 `live`. Three eligible
positions had no question and now have one. `digital-degrowth-book` carries an
attributed span, and its question anchors on the user's own sentence — the
provenance rule holding in the fixture, not only in the gate.

### Still open

- **Density under the answer model.** The §11 reversal above, untested.
- **`register` is unmeasured.** No evidence exists for or against it; §13's
  self-agreement test is the only way to find out.
- **Mode H is still unbuilt.** A live entry explains its silence rather than
  offering a return, so the app justifies its quiet instead of speaking first.
- **No type correction path.** A wrong `role` or `register` is permanent, which
  matters more now that both gate behaviour.
- **Accepted connections have no surface** in the entry panel; only proposed ones
  render.
- **`200 days apart` on onboarding screen 3 is hardcoded** beside hardcoded dates.
  They agree today and nothing keeps them agreeing.
- **Light theme uses `#1D4ED8` for the open-question dot**, not `#D85A30`, so the
  one-colour rule does not hold on that theme.
- **The camera does not fit the corpus on first load**; `tidy` fixes it.
- **Import has no version check.** A pre-facet export loads as `role: undefined` —
  fails closed, but silently wrong.
