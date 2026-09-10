"""Scores the classifier and the probe against a real model.

The prompts are read out of src-tauri/src/enrich/mod.rs rather than copied, so
this measures what ships. Checks mirror what classify() and ask_about() enforce
in code, so a failure here is the model getting it wrong and not the app.

    scripts/fetch-llama.ps1
    # stage a reasoning model, then:
    src-tauri/binaries/llama/llama-server.exe -m <model>.gguf --port 18080 \
        -c 4096 --no-webui --fit-target 256
    python scripts/eval-prompts.py 3

Baseline when written (Qwen3-4B-Q4_K_M, 11 Sep 2026): 12 failures before the
prompts were revised, 0-1 after.
"""

import json
import pathlib
import re
import sys
import time
import urllib.request

URL = "http://127.0.0.1:18080/v1/chat/completions"
TYPES = ["position", "evidence", "note"]
RUST = pathlib.Path(__file__).resolve().parent.parent / "src-tauri/src/enrich/mod.rs"

# Varied on what the gates actually read: role, register and length. Every
# expectation here is a judgement someone can disagree with -- argue with the
# fixture, not with the score.
NOTES = [
    dict(
        id="index",
        want_role="position",
        want_register="neutral",
        text="I keep reaching for an index whenever a query feels slow, but the write "
        "amplification is real and I have never actually measured whether the reads I am "
        "speeding up are the ones anybody waits on.",
    ),
    dict(
        id="p95",
        want_role="evidence",
        want_register="neutral",
        text="The p95 on the search endpoint came back at eight hundred and forty "
        "milliseconds today, which is about triple what it was in March, and the only "
        "thing that changed is the corpus size.",
    ),
    dict(
        id="errands",
        want_role="note",
        want_register="neutral",
        text="Need to renew the domain before the twentieth, email the accountant about "
        "the invoice, and book the dentist some time next week.",
    ),
    dict(
        id="quitting",
        want_role="position",
        want_register="live",
        text="I think I am going to leave, and I cannot tell whether that is courage or "
        "just exhaustion dressed up as a decision, because every reason I give myself "
        "sounds like something I read somewhere.",
    ),
    dict(
        id="freewill",
        want_role="position",
        want_register="neutral",
        text="I do not think free will requires that we could have made a different choice "
        "under exactly the same conditions. It might be enough that the decision came "
        "from reasoning we would endorse on reflection.",
    ),
]

CLASSIFY_SCHEMA = {
    "type": "object",
    "properties": {
        "title": {"type": "string"},
        "role": {"type": "string", "enum": TYPES},
        "register": {"type": "string", "enum": ["live", "neutral"]},
        "typeId": {"type": "string", "enum": TYPES},
        "summary": {"type": "string"},
        "movePhrase": {"type": "string"},
    },
    "required": ["title", "role", "register", "typeId", "summary", "movePhrase"],
    "additionalProperties": False,
}
# quote before text, as in Rust: the span is chosen first and the question is
# written against it.
QUESTION_SCHEMA = {
    "type": "object",
    "properties": {"quote": {"type": "string"}, "text": {"type": "string"}},
    "required": ["quote", "text"],
    "additionalProperties": False,
}


def prompt(name):
    src = RUST.read_text(encoding="utf-8")
    m = re.search(name + r': &str = "\\\n(.*?)";\n', src, re.S)
    if not m:
        sys.exit(f"could not find {name} in {RUST}")
    return re.sub(r"\\\n", "", m.group(1).replace('\\"', '"'))


def ask(system, user, schema, max_tokens):
    body = {
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0.3,
        "max_tokens": max_tokens,
        # Qwen3 otherwise spends the whole budget in reasoning_content and
        # leaves content, which the grammar applies to, empty.
        "chat_template_kwargs": {"enable_thinking": False},
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "reply", "strict": True, "schema": schema},
        },
    }
    req = urllib.request.Request(
        URL, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
    )
    started = time.time()
    got = json.load(urllib.request.urlopen(req, timeout=300))
    return (
        got["choices"][0]["message"].get("content") or "",
        time.time() - started,
    )


def words(text, most):
    """Mirrors trim_title and trim_quote: the app caps these rather than asking."""
    return " ".join(text.split()[:most])


def one_run(classify_system, question_system, show):
    fails, times = [], []

    for note in NOTES:
        out, took = ask(classify_system, note["text"], CLASSIFY_SCHEMA, 700)
        times.append(took)
        try:
            c = json.loads(out)
        except json.JSONDecodeError as e:
            fails.append(f"{note['id']}: classify unparseable ({e})")
            continue

        if c["role"] != note["want_role"]:
            fails.append(f"{note['id']}: role {c['role']!r}, wanted {note['want_role']!r}")
        if c["register"] != note["want_register"]:
            fails.append(
                f"{note['id']}: register {c['register']!r}, wanted {note['want_register']!r}"
            )
        if c["typeId"] != c["role"]:
            fails.append(f"{note['id']}: typeId {c['typeId']!r} disagrees with role {c['role']!r}")
        # §1.1 is enforced in classify(), so a live summary never reaches anyone.
        if c["register"] == "live":
            c["summary"] = ""
        if c["register"] == "neutral" and not c["summary"].strip():
            fails.append(f"{note['id']}: neutral with no summary")
        title = words(c["title"], 4)
        if not 2 <= len(title.split()) <= 4:
            fails.append(f"{note['id']}: title {title!r}")

        # The probe only fires on a neutral position, so only measure it there.
        if note["want_role"] != "position" or note["want_register"] != "neutral":
            continue

        user = f"The note:\n\n{note['text']}\n\nWhat to ask: where does this stop holding?"
        out, took = ask(question_system, user, QUESTION_SCHEMA, 400)
        times.append(took)
        try:
            q = json.loads(out)
        except json.JSONDecodeError as e:
            fails.append(f"{note['id']}: question unparseable ({e})")
            continue

        quote = words(q["quote"], 20)
        if quote not in note["text"]:
            fails.append(f"{note['id']}: quote not verbatim: {quote[:60]!r}")
        if quote and len(quote) > 0.7 * len(note["text"]):
            fails.append(f"{note['id']}: quote is most of the note")
        if "?" not in q["text"]:
            fails.append(f"{note['id']}: question has no question mark")
        if show:
            print(f"  [{note['id']}] {q['text'][:72]}\n          on: {quote[:72]!r}")

    return fails, times


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    classify_system, question_system = prompt("const CLASSIFY_SYSTEM"), prompt(
        "const QUESTION_SYSTEM"
    )

    seen = {}
    for i in range(runs):
        print(f"--- run {i + 1} of {runs}")
        fails, times = one_run(classify_system, question_system, show=True)
        print(f"    {len(fails)} failures, mean {sum(times) / len(times):.2f}s per call")
        for f in fails:
            print("     -", f)
            seen[f] = seen.get(f, 0) + 1

    print(f"\n{sum(seen.values())} failures over {runs} run(s)")
    for text, count in sorted(seen.items(), key=lambda kv: -kv[1]):
        print(f"  {count}x {text}")
    # A run-to-run flip is the model being uncertain, not a regression.
    return 0


if __name__ == "__main__":
    sys.exit(main())
