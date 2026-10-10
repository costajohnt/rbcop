"""Group not-yet-ported cops into porting batches that share RuboCop mixins.

usage: python3 -I scripts/plan_batches.py docs/cop-inventory.tsv <implemented-cops-file> > docs/batches.tsv
Batches never mix departments, prefer cops sharing a mixin, and stay under ~700 Ruby lines or 8 cops.
"""

import csv
import sys

GENERIC = {"AutoCorrector", "RangeHelp", "ConfigurableEnforcedStyle", "TargetRubyVersion", ""}
MAX_LINES, MAX_COPS = 700, 8

rows = list(csv.DictReader(open(sys.argv[1], encoding="utf-8"), delimiter="\t"))
done = set(open(sys.argv[2], encoding="utf-8").read().split())
todo = [r for r in rows if r["cop"] not in done and r["cop"] != "Lint/Syntax"]


def family(r):
    mixins = [m for m in r["mixins"].split(",") if m not in GENERIC]
    return mixins[0] if mixins else "-"


# Order: enabled first, then pending, then disabled; within that by department and family.
rank = {"true": 0, "pending": 1, "false": 2}
todo.sort(key=lambda r: (rank.get(r["enabled"], 3), r["cop"].split("/")[0], family(r), r["cop"]))

batches, cur, cur_lines, cur_key = [], [], 0, None
for r in todo:
    key = (rank.get(r["enabled"], 3), r["cop"].split("/")[0])
    lines = int(r["lines"])
    if cur and (key != cur_key or cur_lines + lines > MAX_LINES or len(cur) >= MAX_COPS):
        batches.append(cur)
        cur, cur_lines = [], 0
    cur.append(r)
    cur_lines += lines
    cur_key = key
if cur:
    batches.append(cur)

print("batch\tstatus\tcops\tlines")
for i, b in enumerate(batches, 1):
    print(f"{i}\t{b[0]['enabled']}\t{','.join(r['cop'] for r in b)}\t{sum(int(r['lines']) for r in b)}")
