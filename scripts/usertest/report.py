"""One table of a Player test run from its parts (docs/USERTEST.md).

Usage: report.py <parts folder> <out folder>. Reads every part's results.json (in the
plan's order, from PARTS), writes the run summary table to GITHUB_STEP_SUMMARY and
results.json plus results.md to the out folder. An item a part did not reach (a job
that failed or was cancelled) is listed as not run.
"""

import json
import os
import sys
from pathlib import Path

NAMES = {"vanilla": "", "fabric": "Fabric", "quilt": "Quilt", "forge": "Forge", "neoforge": "NeoForge"}


def cell(value):
    return str(value if value is not None else "").replace("|", "/").replace("\n", " ")


def timed(state, seconds):
    return f"{state} ({seconds} s)" if seconds else cell(state)


def main():
    parts_dir, out_dir = Path(sys.argv[1]), Path(sys.argv[2])
    out_dir.mkdir(parents=True, exist_ok=True)
    plan = json.loads(os.environ.get("PARTS") or "[]")
    rows = []
    for part in plan:
        items = [i for i in part["items"].split(",") if i]
        found = {}
        path = parts_dir / f"usertest-results-{part['part']}" / "results.json"
        if path.is_file():
            data = json.loads(path.read_text(encoding="utf-8-sig") or "[]")
            for entry in data if isinstance(data, list) else [data]:
                if entry.get("item"):
                    found[entry["item"]] = entry
                elif entry.get("note"):
                    # The launcher itself stopped the part.
                    rows.append({"item": f"(part {part['part']})", "note": entry["note"], "part": part["part"]})
        for item in items:
            loader, _, version = item.partition(":")
            entry = found.get(item) or {"item": item, "loader": loader, "version": version,
                                        "install": "not run", "note": "the part did not get to it"}
            entry["part"] = part["part"]
            rows.append(entry)

    passed = sum(1 for r in rows if r.get("world") == "ok" and r.get("exit") == "ok")
    lines = [
        f"**{passed} of {sum(1 for r in rows if r.get('version'))} passed**",
        "",
        "| Version | Loader | Install | Launch | World | Exit | Log errors | Machine | Note |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in rows:
        ok = r.get("world") == "ok" and r.get("exit") == "ok"
        loader = " ".join(x for x in (NAMES.get(r.get("loader", ""), ""), r.get("loaderVersion") or "") if x)
        exit_state = f"{r.get('exit')} ({r.get('exitCode')})" if r.get("exitCode") is not None else cell(r.get("exit"))
        lines.append("| " + " | ".join([
            ("✅ " if ok else "❌ ") + cell(r.get("version") or r.get("item")),
            cell(loader),
            timed(r.get("install"), r.get("installSeconds")),
            timed(r.get("launch"), r.get("launchSeconds")),
            timed(r.get("world"), r.get("worldSeconds")),
            exit_state,
            cell(r.get("logErrors")),
            cell(r.get("machine")),
            cell(r.get("note")),
        ]) + " |")
    table = "\n".join(lines) + "\n"
    (out_dir / "results.md").write_text(table, encoding="utf-8")
    (out_dir / "results.json").write_text(json.dumps(rows, ensure_ascii=False, indent=1), encoding="utf-8")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(table)
    print(table)


if __name__ == "__main__":
    main()
