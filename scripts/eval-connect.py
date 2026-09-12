"""Measures the judge before anything is wired to it.

Two questions, and the second matters more than the first.

    can it name a connection   over the 12 edges a human authored
    can it decline             over pairs nobody connected

The second is the one to watch. Measured elsewhere in this project, a 4B model
offered an enum does not decline it -- given six tags and nothing fitting, it
picked two anyway. The judge's whole design assumes `none` is easy to say, so
that assumption is tested here rather than discovered in the app.

Accuracy against the authored relations is a weak signal on purpose: 10 of the
12 are `extends`, so answering `extends` to everything scores 83%. The card
that specifies this eval names that failure and cannot catch it; the decline
rate can.

    src-tauri/binaries/llama/llama-server.exe -m <model>.gguf --port 18080 \
        -c 4096 --no-webui --fit-target 256
    python scripts/eval-connect.py

The prompt and the relation list are read out of the Rust rather than copied,
so this measures what ships.
"""

import itertools
import json
import pathlib
import random
import re
import sys
import urllib.request

URL = "http://127.0.0.1:18080/v1/chat/completions"
ROOT = pathlib.Path(__file__).resolve().parent.parent
RUST = ROOT / "src-tauri/src/enrich/connect.rs"
EDGE = ROOT / "src-tauri/src/model/edge.rs"
CORPUS = ROOT / "fixtures/corpus.json"


def prompt():
    """Read the shipped prompt rather than keeping a copy, which would drift.

    Parsed by index rather than by regex: the pattern needs a literal
    backslash-newline, which is the Rust line continuation, and that is the
    one thing that does not survive being retyped.
    """
    src = RUST.read_text(encoding="utf-8")
    marker = 'const CONNECT_SYSTEM: &str = "'
    try:
        i = src.index(marker) + len(marker)
        j = src.index('";', i)
    except ValueError:
        sys.exit(f"could not find CONNECT_SYSTEM in {RUST}")
    body = src[i:j]
    # A backslash at end of line drops the newline and the indent after it.
    return body.replace('\\\n', "").replace('\\"', chr(34))


def relations():
    """The six the model may emit, read from the enum rather than retyped."""
    src = EDGE.read_text(encoding="utf-8")
    block = src[src.index("MODEL_RELATIONS"):src.index("pub fn is_model_emittable")]
    names = re.findall(r"Relation::(\w+)", block)
    serde = dict(re.findall(r'#\[serde\(rename = "([^"]+)"\)\]\s*(\w+),', src))
    by_variant = {v: k for k, v in serde.items()}
    return ["none"] + [by_variant[n] for n in names]


def ask(system, user, allowed):
    body = {
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.3,
        "max_tokens": 500,
        "chat_template_kwargs": {"enable_thinking": False},
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "reply",
                "strict": True,
                "schema": {
                    "type": "object",
                    "properties": {
                        "relation": {"type": "string", "enum": allowed},
                        "quoteA": {"type": "string", "maxLength": 300},
                        "quoteB": {"type": "string", "maxLength": 300},
                        "question": {"type": "string", "maxLength": 300},
                    },
                    "required": ["relation"],
                    "additionalProperties": False,
                },
            },
        },
    }
    req = urllib.request.Request(
        URL, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
    )
    got = json.load(urllib.request.urlopen(req, timeout=300))
    choice = got["choices"][0]
    if choice.get("finish_reason") == "length":
        raise RuntimeError("reply hit the token ceiling -- something is unbounded")
    return json.loads(choice["message"]["content"])


def main():
    corpus = json.loads(CORPUS.read_text(encoding="utf-8"))
    notes = {e["id"]: e for e in corpus["entries"]}
    authored = {frozenset((e["a"], e["b"])): e["relation"] for e in corpus["edges"]}
    system = prompt()
    allowed = relations()
    print(f"relations offered: {allowed}\n")

    def put(a, b):
        return (
            f"First note, {notes[a]['createdAt']}:\n{notes[a]['transcript']}\n\n"
            f"Second note, {notes[b]['createdAt']}:\n{notes[b]['transcript']}"
        )

    def anchor_quote(raw, transcript):
        """Mirrors enrich::connect::anchor_quote -- trim to twenty words, then
        retry once without a final word the cap may have severed."""
        words = (raw or "").split()
        if not words:
            return False
        trimmed = " ".join(words[:20])
        if trimmed in transcript:
            return True
        return len(words[:20]) > 1 and " ".join(words[:19]) in transcript

    def anchored(reply, a, b):
        return anchor_quote(reply.get("quoteA"), notes[a]["transcript"]) and anchor_quote(
            reply.get("quoteB"), notes[b]["transcript"]
        )

    print("the twelve a human authored:")
    named = right = landed = 0
    for pair, want in sorted(authored.items(), key=lambda kv: sorted(kv[0])):
        a, b = sorted(pair)
        r = ask(system, put(a, b), allowed)
        got = r["relation"]
        ok = anchored(r, a, b)
        named += got != "none"
        right += got == want
        landed += got != "none" and ok and bool((r.get("question") or "").strip())
        mark = "=" if got == want else " "
        print(f"  {mark} said {got:<12} wanted {want:<12} anchored {'yes' if ok else 'NO '}  {a} + {b}")

    # Pairs nobody connected. Sampled rather than exhaustive: 108 calls is an
    # hour, and the decline rate does not need that many to be visible.
    random.seed(7)
    others = [p for p in itertools.combinations(sorted(notes), 2) if frozenset(p) not in authored]
    sample = random.sample(others, 12)

    print("\ntwelve nobody connected:")
    declined = 0
    for a, b in sample:
        r = ask(system, put(a, b), allowed)
        got = r["relation"]
        declined += got == "none"
        print(f"  said {got:<12} {a} + {b}")

    n = len(authored)
    print(f"\nauthored: named a relation {named}/{n}, matched it {right}/{n}, "
          f"landed anchored and questioned {landed}/{n}")
    print(f"unconnected: declined {declined}/{len(sample)}")

    faults = []
    if declined == 0:
        faults.append("it never says none -- every pair the filter hands it becomes an edge")
    if named and right / max(named, 1) < 0.25:
        faults.append("it names relations but not the right ones")
    if landed == 0:
        faults.append("nothing anchors, so no proposal can ever land")
    print()
    for f in faults:
        print(f"FAULT  {f}")
    print("healthy" if not faults else f"{len(faults)} fault(s)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
