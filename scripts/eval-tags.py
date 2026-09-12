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
this measures what ships. `normalise` is reimplemented here and is the one
thing that can drift -- keep it matching db/tags.rs.
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


def schema(type_ids, tag_names):
    """Mirrors classify_schema, including the empty-vocabulary case: an empty
    enum is a schema no token satisfies, so the field is omitted instead."""
    properties = {
        "title": {"type": "string"},
        "role": {"type": "string", "enum": ["position", "evidence", "note"]},
        "register": {"type": "string", "enum": ["live", "neutral"]},
        "typeId": {"type": "string", "enum": type_ids},
        "summary": {"type": "string"},
        "movePhrase": {"type": "string"},
        "newTags": {"type": "array", "items": {"type": "string"}},
    }
    if tag_names:
        properties["tags"] = {
            "type": "array",
            "items": {"type": "string", "enum": tag_names},
        }
    return {
        "type": "object",
        "properties": properties,
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
    return json.loads(got["choices"][0]["message"]["content"])


def main():
    notes = json.loads(CORPUS.read_text(encoding="utf-8"))["entries"]
    notes.sort(key=lambda e: e["createdAt"])
    system = prompt("const CLASSIFY_SYSTEM")

    vocabulary = []  # normalised, in the order the corpus coined them
    per_note = {}

    for note in notes:
        reply = ask(system, note["transcript"], schema(["position", "evidence", "note"], vocabulary))

        # Mirrors classify(): normalise both fields, drop duplicates, and fold a
        # coined tag that already exists back into reuse.
        reused = []
        for name in reply.get("tags", []):
            key = normalise(name)
            if key and key not in reused:
                reused.append(key)
        coined = []
        for name in reply.get("newTags", []):
            key = normalise(name)
            if key and key not in reused and key not in coined:
                coined.append(key)

        per_note[note["id"]] = reused + coined
        for key in coined:
            if key not in vocabulary:
                vocabulary.append(key)

        print(f"  {note['id']:<34} reused {reused}  new {coined}")

    counts = {}
    for keys in per_note.values():
        for key in keys:
            counts[key] = counts.get(key, 0) + 1

    total = len(notes)
    pairs = list(itertools.combinations(per_note, 2))
    shared = [(a, b) for a, b in pairs if set(per_note[a]) & set(per_note[b])]
    biggest = max(counts.items(), key=lambda kv: kv[1]) if counts else ("none", 0)
    once = sum(1 for n in counts.values() if n == 1)

    print(f"\n{total} notes, {len(counts)} tags, {sum(counts.values())} assignments")
    print(f"tags used once: {once} of {len(counts)}")
    print(f"largest tag: {biggest[0]} on {biggest[1]} of {total} notes")
    print(f"pairs sharing a tag: {len(shared)} of {len(pairs)}")
    for key, n in sorted(counts.items(), key=lambda kv: -kv[1]):
        print(f"  {n:>2}  {key}")

    share = len(shared) / len(pairs) if pairs else 0
    faults = []
    if biggest[1] / total > BIGGEST_TAG_MAX:
        faults.append(f"monoculture: {biggest[0]} covers {biggest[1]}/{total}")
    if share > HEALTHY_SHARE_MAX:
        faults.append(f"too connected: {share:.0%} of pairs share a tag, filter narrows little")
    if share < HEALTHY_SHARE_MIN:
        faults.append(f"drift: only {share:.0%} of pairs share a tag, little will connect")

    print()
    for fault in faults:
        print(f"FAULT  {fault}")
    print("healthy" if not faults else f"{len(faults)} fault(s)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
