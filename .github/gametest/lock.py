"""Locks the medium mod sets that passed (docs/GAMETEST.md).

The Game test harness writes lock.json in a scenario's folder when the newest mod
versions passed: the game version, the loader and its build, and each mod's title
and Modrinth version id. This takes the gate's and the newest-mods run's ones
(the latter win for the same version and loader), asks Modrinth for each version's
file, and writes the locked sets file, one mod file per line.

    python3 -I lock.py <locked sets file> <gate's folder> <newest run's folder> <summary.md>

Everything read is checked against fixed patterns; a set with anything else in
it is left as it was. Only fixed words, game versions, loader builds and public
mod names and versions are printed: the logs are public.
"""

import json
import pathlib
import re
import sys
import urllib.parse
import urllib.request

API = "https://api.modrinth.com/v2"
MAX_FILES = 40
MAX_FRAGMENT = 64 * 1024
MAX_SIZE = 256 * 1024 * 1024

GAME = re.compile(r"\d{1,2}\.\d{1,2}(\.\d{1,2})?")
LOADERS = ("fabric", "quilt", "neoforge", "forge")
BUILD = re.compile(r"[A-Za-z0-9._+-]{1,64}")
TITLE = re.compile(r"[A-Za-z0-9 ()._+-]{1,200}")
ID = re.compile(r"[A-Za-z0-9]{1,32}")
VERSION = re.compile(r"[\x20-\x7e]{1,100}")
FILE_NAME = re.compile(r"[A-Za-z0-9._+-]{1,124}\.jar")
URL = re.compile(r"https://cdn\.modrinth\.com/[A-Za-z0-9._~%+/-]{1,480}")
SHA512 = re.compile(r"[0-9a-f]{128}")
SHA1 = re.compile(r"[0-9a-f]{40}")


def ok(pattern, value):
    return isinstance(value, str) and pattern.fullmatch(value) is not None


def fragments(folder):
    """The sets in a folder's lock.json files, by "<game> <loader>"."""
    found = {}
    root = pathlib.Path(folder)
    for path in sorted(root.rglob("lock.json")) if root.is_dir() else []:
        if path.stat().st_size > MAX_FRAGMENT:
            print("A lock.json is too large: left out.")
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            print("A lock.json does not read: left out.")
            continue
        files = data.get("files") if isinstance(data, dict) else None
        if not (
            ok(GAME, data.get("game"))
            and data.get("loader") in LOADERS
            and ok(BUILD, data.get("loaderVersion"))
            and isinstance(files, list)
            and 1 <= len(files) <= MAX_FILES
            and all(
                isinstance(f, dict) and ok(TITLE, f.get("title")) and ok(ID, f.get("versionId"))
                for f in files
            )
            and len({f["versionId"] for f in files}) == len(files)
        ):
            print("A lock.json is not as the harness writes it: left out.")
            continue
        key = f"{data['game']} {data['loader']}"
        found[key] = {
            "loader": data["loaderVersion"],
            "files": [{"title": f["title"], "versionId": f["versionId"]} for f in files],
        }
    return found


def modrinth(ids):
    """Modrinth's versions by id."""
    out = {}
    ids = sorted(ids)
    for i in range(0, len(ids), 100):
        query = urllib.parse.urlencode({"ids": json.dumps(ids[i : i + 100])})
        request = urllib.request.Request(
            f"{API}/versions?{query}",
            headers={"User-Agent": "kurasis/launcher game test (github.com/kurasis/launcher)"},
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            body = response.read(16 * 1024 * 1024)
        for version in json.loads(body):
            if isinstance(version, dict) and ok(ID, version.get("id")):
                out[version["id"]] = version
    return out


def locked_file(title, version):
    """A version's file as the harness reads it from the lock; None when anything
    in it is not as expected."""
    if not isinstance(version, dict):
        return None
    files = version.get("files")
    if not isinstance(files, list) or not files:
        return None
    file = next((f for f in files if isinstance(f, dict) and f.get("primary") is True), files[0])
    if not isinstance(file, dict):
        return None
    hashes = file.get("hashes") if isinstance(file.get("hashes"), dict) else {}
    out = {
        "title": title,
        "version": version.get("version_number"),
        "fileName": file.get("filename"),
        "project": version.get("project_id"),
        "versionId": version.get("id"),
        "url": file.get("url"),
        "sha512": str(hashes.get("sha512", "")).lower(),
        "sha1": str(hashes.get("sha1", "")).lower(),
        "size": file.get("size"),
    }
    good = (
        ok(VERSION, out["version"])
        and ok(FILE_NAME, out["fileName"])
        and ok(ID, out["project"])
        and ok(URL, out["url"])
        and ok(SHA512, out["sha512"])
        and ok(SHA1, out["sha1"])
        and isinstance(out["size"], int)
        and 0 < out["size"] <= MAX_SIZE
    )
    return out if good else None


def top(entry):
    """The titles of the mods asked for, not the libraries they brought."""
    return {
        f.get("title")
        for f in entry["files"]
        if isinstance(f, dict) and " (" not in str(f.get("title"))
    }


def write(path, sets):
    lines = ["{"]
    keys = sorted(sets)
    for k, key in enumerate(keys):
        entry = sets[key]
        lines.append(f"  {json.dumps(key)}: {{")
        lines.append(f'    "loader": {json.dumps(entry["loader"])},')
        lines.append('    "files": [')
        for i, f in enumerate(entry["files"]):
            comma = "," if i < len(entry["files"]) - 1 else ""
            lines.append(f"      {json.dumps(f, separators=(', ', ': '))}{comma}")
        lines.append("    ]")
        lines.append("  }" + ("," if k < len(keys) - 1 else ""))
    lines.append("}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def changes(old, new):
    """What changed in one set, in public names and versions."""
    if old is None:
        return [f"new: {len(new['files'])} files on {new['loader']}"]
    old = {"loader": old.get("loader"), "files": [f for f in old["files"] if isinstance(f, dict)]}
    out = []
    if old["loader"] != new["loader"]:
        out.append(f"loader {old['loader']} -> {new['loader']}")
    before = {f.get("title"): f.get("version") for f in old["files"]}
    after = {f["title"]: f["version"] for f in new["files"]}
    for title in sorted(before.keys() | after.keys()):
        if title not in after:
            out.append(f"- {title} {before[title]}")
        elif title not in before:
            out.append(f"+ {title} {after[title]}")
        elif before[title] != after[title]:
            out.append(f"{title} {before[title]} -> {after[title]}")
    return out


def main():
    if len(sys.argv) != 5:
        sys.exit("usage: lock.py <locked sets file> <gate's folder> <newest run's folder> <summary.md>")
    lock_path = pathlib.Path(sys.argv[1])
    sets = json.loads(lock_path.read_text(encoding="utf-8"))
    if not isinstance(sets, dict):
        sys.exit("The locked sets file is not an object.")
    passed = fragments(sys.argv[2])
    passed.update(fragments(sys.argv[3]))
    if not passed:
        print("No set passed with the newest versions: nothing to lock.")
        return
    versions = modrinth({f["versionId"] for entry in passed.values() for f in entry["files"]})
    report = []
    for key in sorted(passed):
        entry = passed[key]
        files = [locked_file(f["title"], versions.get(f["versionId"])) for f in entry["files"]]
        if not all(files):
            print(f"{key}: a version is not on Modrinth as expected: left as it was.")
            continue
        new = {"loader": entry["loader"], "files": files}
        old = sets.get(key)
        if not (isinstance(old, dict) and isinstance(old.get("files"), list)):
            old = None
        if old is not None and not top(old) <= top(new):
            print(f"{key}: the set that passed lacks mods the locked one has: left as it was.")
            continue
        changed = changes(old, new)
        if old is not None and not changed:
            continue
        sets[key] = new
        print(f"{key}: {'; '.join(changed)}")
        report.append(f"| {key} | {'<br>'.join(changed)} |")
    if not report:
        print("The locked sets stay as they are.")
        return
    write(lock_path, sets)
    text = "### Locked mod sets\n\n| Set | Changes |\n| --- | --- |\n" + "\n".join(report) + "\n"
    pathlib.Path(sys.argv[4]).write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
