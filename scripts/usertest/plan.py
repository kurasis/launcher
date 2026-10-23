"""What the Player test plays, cut into parts (docs/USERTEST.md).

Reads VERSIONS (comma-separated: "26", "all", "vanilla", a loader name, a release id
or loader:id, "special", special:<group> or special:<id>), PART_SIZE and CLEANUP from
the environment and writes the job matrix to GITHUB_OUTPUT as `parts`:
[{"part": 1, "items": "vanilla:26.3,fabric:26.3,..."}].

Game versions are Mojang releases, newest first; for each one the loaders follow in
the form's order. Which releases a loader supports comes from the same lists the
launcher reads (src-tauri/src/loader/meta.rs), with the same rules. The special
versions (the launcher's own list on upd.lapi.sh, docs/EXTRA-VERSIONS.md) follow in
the list's order; what the form shows for each goes to SPECIAL_FILE.
"""

import json
import os
import re
import sys
import urllib.request

LOADERS = ["vanilla", "fabric", "quilt", "forge", "neoforge"]
MAX_PARTS = 64
# The launcher's own list of versions Mojang's list does not have (D-073).
SPECIAL_LIST = "https://upd.lapi.sh/versions/manifest.json"
SPECIAL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9 ._-]{0,63}")


def get(url):
    request = urllib.request.Request(url, headers={"User-Agent": "launcher-player-test"})
    with urllib.request.urlopen(request, timeout=60) as answer:
        return json.load(answer)


def numbers(version):
    return [int(p) if p.isdigit() else 0 for p in re.split(r"[.\-]", version)]


def at_least(version, first):
    a, b = numbers(version), numbers(first)
    width = max(len(a), len(b))
    return a + [0] * (width - len(a)) >= b + [0] * (width - len(b))


def forge_supported(game, build):
    """meta.rs forge_supported: installers with a version profile, and the last
    builds of 1.7.10 and 1.8.9."""
    prefix = f"{game}-"
    if not build.startswith(prefix):
        return False
    version = build[len(prefix):]
    legacy = {"1.7.10": "10.13.4.1614", "1.8.9": "11.15.1.2318"}.get(game)
    if legacy:
        return at_least(version.split("-")[0], legacy)
    if game == "1.12.2":
        return at_least(version, "14.23.5.2851")
    parts = game.split(".")
    if parts[0] == "1" and len(parts) > 1 and parts[1].isdigit():
        return int(parts[1]) >= 13
    return parts[0].isdigit() and int(parts[0]) >= 25


def neoforge_game(version):
    """meta.rs neoforge_game: 21.1.262 is for 1.21.1, 26.1.2.122 for 26.1.2."""
    if "+" in version:
        return None
    try:
        parts = [int(p) for p in version.split("-")[0].split(".")]
    except ValueError:
        return None
    if len(parts) == 3 and 20 <= parts[0] <= 24:
        return f"1.{parts[0]}" if parts[1] == 0 else f"1.{parts[0]}.{parts[1]}"
    if len(parts) == 4 and 25 <= parts[0] <= 99:
        return f"{parts[0]}.{parts[1]}" if parts[2] == 0 else f"{parts[0]}.{parts[1]}.{parts[2]}"
    return None


def supported(loader):
    if loader == "fabric":
        return {v["version"] for v in get("https://meta.fabricmc.net/v2/versions/game")}
    if loader == "quilt":
        return {v["version"] for v in get("https://meta.quiltmc.org/v3/versions/game")}
    if loader == "forge":
        meta = get("https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json")
        return {game for game, builds in meta.items() if any(forge_supported(game, b) for b in builds)}
    if loader == "neoforge":
        base = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged"
        games = {g for g in map(neoforge_game, get(f"{base}/neoforge")["versions"]) if g}
        if get(f"{base}/forge")["versions"]:
            games.add("1.20.1")
        return games
    raise ValueError(loader)


def special_versions():
    """The list's versions in its order: April Fools versions newest first, then the
    rest. Each with the name its row starts with and what the form's "Selected" line
    shows: the name, or the id of an earlier build (CreateInstanceForm.tsx)."""
    listed = json.loads(get(SPECIAL_LIST)["list"])["versions"]
    versions = []
    for v in listed:
        if not SPECIAL_ID.fullmatch(v["id"]) or not 0 < len(v["name"]) <= 64:
            sys.exit(f"the special versions list has an entry the test cannot use: {v['id']!r}")
        selected = v["id"] if v.get("variant") == "first-build" else v["name"]
        versions.append({"id": v["id"], "group": v["group"], "name": v["name"],
                         "selected": selected, "note": v.get("note")})
    return versions


def main():
    out = os.environ["GITHUB_OUTPUT"]
    if os.environ.get("CLEANUP") == "yes":
        with open(out, "a") as f:
            f.write('parts=[{"part": 1, "items": ""}]\n')
        return
    try:
        size = int(os.environ.get("PART_SIZE", "12"))
    except ValueError:
        sys.exit("part_size: a whole number")
    if not 1 <= size <= 100:
        sys.exit("part_size: 1 to 100")

    manifest = get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
    releases = sorted(
        (v for v in manifest["versions"] if v["type"] == "release"),
        key=lambda v: v["releaseTime"],
        reverse=True,
    )
    ids = [v["id"] for v in releases]
    rank = {v: i for i, v in enumerate(ids)}
    lists = {"vanilla": set(ids)}

    def games(loader):
        if loader not in lists:
            lists[loader] = supported(loader) & set(ids)
        return lists[loader]

    picked = set()
    specials = None
    special_picked = set()
    for token in (t.strip() for t in os.environ["VERSIONS"].split(",")):
        if not token:
            continue
        if token == "special" or token.startswith("special:"):
            if specials is None:
                specials = special_versions()
            wanted = token.partition(":")[2]
            # A build known to crash at start (its note) is played only by its id.
            chosen = {v["id"] for v in specials
                      if wanted == v["id"] or (wanted in ("", v["group"]) and not v["note"])}
            if not chosen:
                sys.exit(f"versions: {wanted!r} is no group or id of the special versions")
            special_picked |= chosen
        elif token == "26":
            picked |= {("vanilla", v) for v in ids if re.fullmatch(r"26\.\d+(\.\d+)?", v)}
        elif token == "all":
            picked |= {(loader, v) for loader in LOADERS for v in games(loader)}
        elif token in LOADERS:
            picked |= {(token, v) for v in games(token)}
        else:
            loader, _, version = token.rpartition(":")
            loader = loader or "vanilla"
            if loader not in LOADERS:
                sys.exit(f"versions: {loader!r} is not a loader")
            if version not in rank:
                sys.exit(f"versions: {version!r} is not a Mojang release")
            if version not in games(loader):
                sys.exit(f"versions: {loader} has no build for {version}")
            picked.add((loader, version))
    if not picked and not special_picked:
        sys.exit("versions: none picked")

    items = sorted(picked, key=lambda p: (rank[p[1]], LOADERS.index(p[0])))
    shown = [v for v in specials or [] if v["id"] in special_picked]
    items += [("special", v["id"]) for v in shown]
    special_file = os.environ.get("SPECIAL_FILE")
    if special_file:
        with open(special_file, "w", encoding="utf-8") as f:
            json.dump({v["id"]: {"name": v["name"], "selected": v["selected"]} for v in shown},
                      f, ensure_ascii=False)
    size = max(size, -(-len(items) // MAX_PARTS))
    parts = [
        {"part": n + 1, "items": ",".join(f"{l}:{v}" for l, v in items[i:i + size])}
        for n, i in enumerate(range(0, len(items), size))
    ]
    with open(out, "a") as f:
        f.write(f"parts={json.dumps(parts)}\n")
    counts = {loader: sum(1 for l, _ in items if l == loader) for loader in LOADERS + ["special"]}
    print(f"{len(items)} items in {len(parts)} parts:", ", ".join(f"{k} {n}" for k, n in counts.items() if n))
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as f:
            f.write(f"Plan: {len(items)} items in {len(parts)} parts ("
                    + ", ".join(f"{k} {n}" for k, n in counts.items() if n) + ")\n\n")


if __name__ == "__main__":
    main()
