#!/usr/bin/env python3
"""Builds the magnesium ranking prototype from the generator's output.

The prototype renders only what `site-gen` computed: the evaluation files
under design/prototypes/cases/ are written by
`UPDATE_PROTOTYPE_CASES=1 cargo test -p site-gen --test evaluation`
and checked by that test on every run (ADR 0020).

    python3 tools/build-prototype.py                 # the committed prototype
    python3 tools/build-prototype.py --case one-compared --out /tmp/one.html
    python3 tools/build-prototype.py --fragment --notes notes.html --out page.html

Without --case the page carries fixture-a (ratings allowed) and fixture-b
(ratings not allowed) and can switch between them. --fragment leaves out the
<!doctype> wrapper, for hosts that add their own.
"""

import argparse
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PROTO = ROOT / "design" / "prototypes"
CASES = PROTO / "cases"
WRAP = (
    '<!doctype html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
    '<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">\n'
    "<style>body{margin:0}[hidden]{display:none!important}</style>\n</head>\n<body>\n"
)


def load(name):
    path = CASES / f"{name}.json"
    if not path.exists():
        sys.exit(f"{path} is missing; run UPDATE_PROTOTYPE_CASES=1 cargo test -p site-gen --test evaluation")
    return json.loads(path.read_text())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--case", help="one case from design/prototypes/cases instead of fixture-a and fixture-b")
    ap.add_argument("--out", default=str(PROTO / "magnesium-ranking.html"))
    ap.add_argument("--notes", default="../../docs/guidelines/art-direction.md", help="href of the design notes link")
    ap.add_argument("--fragment", action="store_true", help="no doctype wrapper")
    args = ap.parse_args()

    if args.case:
        one = load(args.case)
        sets = {"A": one if one["branch"] == "A" else None, "B": one if one["branch"] == "B" else None}
    else:
        sets = {"A": load("fixture-a"), "B": load("fixture-b")}
    data = json.dumps(sets, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")
    src = (PROTO / "magnesium-ranking.src.html").read_text()
    for marker in ("/*CASES*/", "/*NOTES*/"):
        if src.count(marker) != 1:
            sys.exit(f"magnesium-ranking.src.html must contain {marker} once")
    page = src.replace("/*CASES*/", data).replace("/*NOTES*/", args.notes)
    if not args.fragment:
        page = WRAP + page + "\n</body>\n</html>\n"
    pathlib.Path(args.out).write_text(page)
    print(f"{args.out}: {len(page)} bytes")


if __name__ == "__main__":
    main()
