"""Measures the tag vocabulary a real model produces, which decides whether
the connection filter filters anything at all.

Tags are the candidate filter for proposed connections: a new note is only ever
compared against notes it shares a tag with. Two failures make that useless and
neither is visible in the code.

    drift        every note coins its own spelling, no two notes share a key,
                 nothing is ever a candidate
    monoculture  the model is agreeable, reuses everything, one tag ends up on
                 most of the corpus, and the filter selects all of it

Run it against the shipped fixtures, in created_at order, because that is how a
corpus really grows -- early notes seed the vocabulary later ones are offered.

    src-tauri/binaries/llama/llama-server.exe -m <model>.gguf --port 18080 \
        -c 4096 --no-webui --fit-target 256
    python scripts/eval-tags.py

The prompt and the type list are read out of the Rust rather than copied, so
this measures what ships. `normalise` and `grounded` are reimplemented here and
are the things that can drift -- keep them matching db/tags.rs and enrich/mod.rs.

The last number is the one that decides anything: of the edges a human authored
in the fixture, how many does the filter still surface? A pair that shares no
tag is never compared, so a dropped edge is a connection lost for good.
"""

import itertools
import json
import pathlib
import re
import sys
import urllib.request

URL = "http://127.0.0.1:18080/v1/chat/completions"
ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST = ROOT / "src-tauri/src/enrich/mod.rs"
CORPUS = ROOT / "fixtures/corpus.json"

# What a healthy vocabulary looks like on a corpus this size. Argue with these
# numbers rather than with the score.
HEALTHY_SHARE_MIN = 0.15  # at least this fraction of pairs share a tag
HEALTHY_SHARE_MAX = 0.60  # and not more than this, or nothing is narrowed
BIGGEST_TAG_MAX = 0.50  # no tag on more than half the corpus


def prompt(name):
    src = RUST.read_text(encoding="utf-8")
    m = re.search(name + r': &str = "\\\n(.*?)";\n', src, re.S)
    if not m:
        sys.exit(f"could not find {name} in {RUST}")
    return re.sub(r"\\\n", "", m.group(1).replace('\\"', '"'))


def normalise(name):
    """Mirrors db::tags::normalise. Lowercase, separators collapsed to one
    hyphen, no leading or trailing hyphen."""
    out = []
    gap = False
    for ch in name.strip():
        if ch.isspace() or ch in "-_":
            gap = bool(out)
            continue
        if gap:
            out.append("-")
            gap = False
        out.append(ch.lower())
    return "".join(out)


def grounded(tag, transcript):
    """Mirrors enrich::grounded. A tag whose words are not in the note is
    dropped: measured, the model coined "philosophy" and "decision-making" on
    the first fixture and put them on all sixteen, and neither is in the text.
    Five-character stem so "index" still finds "indexes"."""
    haystack = transcript.lower()
    words = [w for w in tag.split("-") if len(w) > 2]
    return bool(words) and all(w[:5] in haystack for w in words)


def schema(type_ids):
    """Mirrors classify_schema. The vocabulary is deliberately not offered:
    measured, an enum of existing tags stopped the model coining from note two
    onward and froze the corpus at one tag. Reuse happens in db::tags::upsert,
    by normalised collision."""
    return {
        "type": "object",
        "properties": {
            "title": {"type": "string", "maxLength": 80},
            "role": {"type": "string", "enum": ["position", "evidence", "note"]},
            "register": {"type": "string", "enum": ["live", "neutral"]},
            "typeId": {"type": "string", "enum": type_ids},
            "summary": {"type": "string", "maxLength": 400},
            # Bounded because the model loops inside an unbounded string until
            # the token ceiling and the JSON never terminates.
            "movePhrase": {"type": "string", "maxLength": 200},
            "tags": {"type": "array", "items": {"type": "string"}, "maxItems": 3},
        },
        "required": ["title", "role", "register", "typeId", "summary", "movePhrase"],
        "additionalProperties": False,
    }


def ask(system, user, json_schema):
    body = {
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.3,
        "max_tokens": 700,
        # Qwen3 otherwise spends the whole budget in reasoning_content and
        # leaves content, which the grammar applies to, empty.
        "chat_template_kwargs": {"enable_thinking": False},
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "reply", "strict": True, "schema": json_schema},
        },
    }
    req = urllib.request.Request(
        URL, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
    )
    got = json.load(urllib.request.urlopen(req, timeout=300))
    choice = got["choices"][0]
    if choice.get("finish_reason") == "length":
        # Worth failing loudly rather than skipping: a truncated reply in the
        # app is an unparseable classification, not a missing tag.
        raise RuntimeError("reply hit the token ceiling -- the grammar is unbounded somewhere")
    return json.loads(choice["message"]["content"])


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    notes = corpus["entries"]
    notes.sort(key=lambda e: e["createdAt"])
    system = prompt("const CLASSIFY_SYSTEM")

    per_note = {}
    for note in notes:
        reply = ask(system, note["transcript"], schema(["position", "evidence", "note"]))

        # Mirrors classify(): normalise, drop duplicates, drop what the note
        # does not actually say.
        said = note["transcript"]
        keys = []
        for name in reply.get("tags", []):
            key = normalise(name)
            if key and key not in keys and grounded(key, said):
                keys.append(key)
        per_note[note["id"]] = keys
        print(f"  {note['id']:<34} {keys}")

    counts = {}
    for keys in per_note.values():
        for key in keys:
            counts[key] = counts.get(key, 0) + 1

    total = len(notes)
    pairs = list(itertools.combinations(per_note, 2))
    shared = {frozenset(p) for p in pairs if set(per_note[p[0]]) & set(per_note[p[1]])}
    biggest = max(counts.items(), key=lambda kv: kv[1]) if counts else ("none", 0)
    once = sum(1 for n in counts.values() if n == 1)

    print()
    print(f"{total} notes, {len(counts)} tags, {sum(counts.values())} assignments")
    print(f"tags used once: {once} of {len(counts)}")
    print(f"largest tag: {biggest[0]} on {biggest[1]} of {total} notes")
    print(f"pairs sharing a tag: {len(shared)} of {len(pairs)}")
    for key, n in sorted(counts.items(), key=lambda kv: -kv[1]):
        print(f"  {n:>2}  {key}")

    # The number that decides it. `pairs sharing a tag` is a proxy chosen
    # against invented thresholds; the corpus carries authored edges, and a
    # pair the filter drops is never judged and is lost for good.
    truth = {frozenset((e["a"], e["b"])) for e in corpus.get("edges", [])}
    kept = truth & shared
    if truth:
        print()
        print(f"authored edges surfaced: {len(kept)} of {len(truth)}")
        for edge in sorted(truth - kept, key=sorted):
            a, b = sorted(edge)
            print(f"  MISSED  {a} + {b}")
            print(f"          {per_note.get(a)}  vs  {per_note.get(b)}")

    share = len(shared) / len(pairs) if pairs else 0
    faults = []
    if biggest[1] / total > BIGGEST_TAG_MAX:
        faults.append(f"monoculture: {biggest[0]} covers {biggest[1]}/{total}")
    if share > HEALTHY_SHARE_MAX:
        faults.append(f"too connected: {share:.0%} of pairs share a tag, filter narrows little")
    if share < HEALTHY_SHARE_MIN:
        faults.append(f"drift: only {share:.0%} of pairs share a tag, little will connect")
    if truth and len(kept) < len(truth) / 2:
        faults.append(f"the filter loses real connections: {len(kept)}/{len(truth)} authored edges survive")

    print()
    for fault in faults:
        print(f"FAULT  {fault}")
    print("healthy" if not faults else f"{len(faults)} fault(s)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
