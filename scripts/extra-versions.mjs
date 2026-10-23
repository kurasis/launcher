#!/usr/bin/env node
// Writes data/extra-versions.json: the April Fools versions and every other
// Minecraft version that Mojang's version list does not have (docs/EXTRA-VERSIONS.md).
//
//   node scripts/extra-versions.mjs
//
// The joke versions come first, newest first, then the rest, newest first.
// Joke versions in Mojang's list are only named here; the launcher installs
// them from that list. Every other version gets a version file of its own,
// pinned by SHA-1: builds on Mojang's servers keep the version file the
// archive gives them (or the file of the Mojang build they replaced), and the
// archived old ones take the file of the nearest older Mojang version, so they
// start the way Mojang starts that era. Their game files come from the archive
// list at skyrising.github.io/mc-versions; versions whose client is the same
// file as one in Mojang's list, and servers without a client, are left out.
//
// Every version file names its game jars at upd.lapi.sh, where the
// Versions workflow puts them after a virus check (D-073): the launcher reaches
// no other host for these versions but Mojang's libraries, assets and Java,
// as for every version. `files` keeps where each jar came from.
//
// It also writes data/extra-world-versions.txt: the ids of those a world with
// friends can be made for, one per line (those with a server, from 1.7.6's day
// on), which the worlds service takes for rooms.
//
// Network: Mojang's version list and files, the archive list. With a proxy:
// NODE_USE_ENV_PROXY=1. It changes only these two files.
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';

const MANIFEST = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json';
const ARCHIVE = 'https://skyrising.github.io/mc-versions/version_manifest.json';
const OUT = new URL('../data/extra-versions.json', import.meta.url);
const WORLDS_OUT = new URL('../data/extra-world-versions.txt', import.meta.url);
// 1.7.6's day, as the launcher's world versions (src-tauri/src/worlds/tuple.rs).
const WORLDS_FROM = Date.parse('2014-04-09T00:00:00Z');
/** Where launchers download the game jars (services/upd/src/versions.rs). */
const FILES = 'https://upd.lapi.sh/versions/files/';

/** Hosts a published version file may name: Mojang's, as for every version, and ours. */
const HOSTS = new Set([
  'piston-meta.mojang.com',
  'piston-data.mojang.com',
  'launcher.mojang.com',
  'launchermeta.mojang.com',
  'libraries.minecraft.net',
  'resources.download.minecraft.net',
  'upd.lapi.sh',
]);
/** Where CI may fetch a game jar from: Mojang and the two archives. */
const ORIGINS = new Set([
  'piston-data.mojang.com',
  'launcher.mojang.com',
  'files.betacraft.uk',
  'vault.omniarchive.uk',
]);

/** The April Fools versions in Mojang's list, newest first. */
const MANIFEST_JOKES = [
  { id: '26w14a', title: 'Herdcraft Update' },
  { id: '25w14craftmine', title: 'Craftmine Update' },
  { id: '24w14potato', title: 'Poisonous Potato Update' },
  { id: '23w13a_or_b', title: 'The Vote Update' },
  { id: '22w13oneblockatatime', name: '22w13oneBlockAtATime', title: 'One Block at a Time' },
  { id: '20w14infinite', name: '20w14∞', title: 'Infinite Dimensions' },
  { id: '3D Shareware v1.34', title: 'Minecraft 3D' },
  { id: '1.RV-Pre1', title: 'Trendy Update' },
  { id: '15w14a', title: 'Love and Hugs Update' },
];

/**
 * Archive ids renamed: a launcher id is a folder name (letters, digits, `.`,
 * `_`, `-`, spaces), and a joke build gets its own name.
 */
const RENAMED = {
  'af-2013-blue': { id: '2.0-blue', name: '2.0 Blue', title: 'Minecraft 2.0', group: 'joke' },
  'af-2013-red': { id: '2.0-red', name: '2.0 Red', title: 'Minecraft 2.0', group: 'joke' },
  'af-2013-purple': { id: '2.0-purple', name: '2.0 Purple', title: 'Minecraft 2.0', group: 'joke' },
  'af-2023-1': {
    id: '23w13a_or_b-first',
    name: '23w13a_or_b',
    title: 'The Vote Update',
    group: 'joke',
    variant: 'first-build',
    template: '23w13a_or_b',
  },
  'af-2024': {
    id: '24w14potato-first',
    name: '24w14potato',
    title: 'Poisonous Potato Update',
    group: 'joke',
    variant: 'first-build',
    template: '24w14potato',
  },
  '13w12~': { id: '13w12_', name: '13w12~' },
  1.2: { id: '1.2-pre', name: '1.2-pre' },
};

/** Earlier builds of versions in Mojang's list, which Mojang replaced under the same name. */
const FIRST_BUILDS = {
  '1.16-1149': '1.16',
  '1.16.5-rc1-1': '1.16.5-rc1',
  '1.14.2-pre4-241548': '1.14.2 Pre-Release 4',
  '1.12-pre3-1316': '1.12-pre3',
};

/** Archive addresses that do not answer, by the jar's SHA-1: the file is at another name. */
const ORIGIN_FIXES = {
  '3522a5de5641e90199b835e02f05aa0fb3d0ef5d':
    'https://vault.omniarchive.uk/archive/java/client-indev/in-20100131-2244.jar',
};

/** The first Mojang version started by `net.minecraft.client.main.Main`. */
const MODERN_FROM = '2013-04-21';

const sha1 = (text) => createHash('sha1').update(text).digest('hex');

async function json(url) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${response.status} ${url}`);
      return await response.json();
    } catch (error) {
      if (attempt === 4) throw error;
      await new Promise((resolve) => setTimeout(resolve, 1000 * attempt));
    }
  }
}

async function each(items, work, atOnce = 16) {
  const results = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: atOnce }, async () => {
      while (next < items.length) {
        const index = next;
        next += 1;
        results[index] = await work(items[index]);
      }
    }),
  );
  return results;
}

function group(id) {
  if (id.startsWith('combat-')) return 'combat';
  if (id.includes('experimental')) return 'experimental';
  if (id.endsWith('_unobfuscated')) return 'unobfuscated';
  if (id in FIRST_BUILDS) return 'first-build';
  if (id.startsWith('c0.')) return 'classic';
  if (id.startsWith('in-')) return 'indev';
  if (id.startsWith('inf-')) return 'infdev';
  if (id.startsWith('a1.')) return 'alpha';
  if (id.startsWith('b1.')) return 'beta';
  return 'early-release';
}

function name(id, kind) {
  if (kind === 'combat') {
    const test = id.slice('combat-'.length);
    return `Combat Test ${test === '7a' ? '7' : test}`;
  }
  if (id === '1.19_deep_dark_experimental_snapshot-1') {
    return '1.19 Deep Dark Experimental Snapshot 1';
  }
  if (kind === 'experimental') {
    return `1.18 Experimental Snapshot ${id.split('-').at(-1)}`;
  }
  if (kind === 'unobfuscated') return `${id.slice(0, -'_unobfuscated'.length)} Unobfuscated`;
  if (kind === 'first-build') return FIRST_BUILDS[id];
  return id;
}

/** Every URL a version file names. */
function urls(descriptor) {
  const found = [];
  for (const download of Object.values(descriptor.downloads ?? {})) found.push(download.url);
  for (const library of descriptor.libraries ?? []) {
    if (library.downloads?.artifact) found.push(library.downloads.artifact.url);
    for (const native of Object.values(library.downloads?.classifiers ?? {})) {
      found.push(native.url);
    }
  }
  if (descriptor.assetIndex) found.push(descriptor.assetIndex.url);
  if (descriptor.logging?.client?.file) found.push(descriptor.logging.client.file.url);
  return found;
}

function checkHosts(id, descriptor) {
  for (const url of urls(descriptor)) {
    const parsed = new URL(url);
    if (parsed.protocol !== 'https:' || !HOSTS.has(parsed.hostname) || parsed.port !== '') {
      throw new Error(`${id}: unexpected address ${url}`);
    }
  }
}

function checkDownload(id, download) {
  if (!/^[0-9a-f]{40}$/.test(download?.sha1 ?? '') || !(download.size > 0)) {
    throw new Error(`${id}: a download without SHA-1 or size`);
  }
}

/**
 * The version file as launchers get it: its game jars at upd.lapi.sh, by
 * SHA-1; `files` says where CI fetches each one before the virus check.
 */
function published(id, descriptor) {
  // Mojang's current log configuration of that name: the archived builds name
  // the one from before Mojang's Log4Shell fix of December 2021.
  const logging = descriptor.logging?.client?.file?.id;
  if (logging) {
    if (!LOGGING.has(logging)) throw new Error(`${id}: unknown log configuration ${logging}`);
    descriptor.logging = structuredClone(LOGGING.get(logging));
  }
  const files = [];
  for (const [kind, download] of Object.entries(descriptor.downloads)) {
    checkDownload(id, download);
    const origin = new URL(ORIGIN_FIXES[download.sha1] ?? download.url);
    if (origin.protocol !== 'https:' || !ORIGINS.has(origin.hostname)) {
      throw new Error(`${id}: a game jar from ${download.url}`);
    }
    files.push({ kind, sha1: download.sha1, size: download.size, origin: origin.href });
    download.url = `${FILES}${download.sha1}.jar`;
  }
  checkHosts(id, descriptor);
  const text = JSON.stringify(descriptor);
  return { server: 'server' in descriptor.downloads, sha1: sha1(text), files, descriptor: text };
}

const mojang = await json(MANIFEST);
const mojangVersions = await each(mojang.versions, async (entry) => ({
  entry,
  descriptor: await json(entry.url),
}));
const byId = new Map(mojangVersions.map((v) => [v.entry.id, v]));
const mojangIds = new Set(mojang.versions.map((v) => v.id.toLowerCase()));
const mojangClients = new Set(
  mojangVersions.map((v) => v.descriptor.downloads?.client?.sha1).filter(Boolean),
);
/** The newest Mojang `logging` block per log configuration name. */
const LOGGING = new Map();
for (const { descriptor } of mojangVersions) {
  const name = descriptor.logging?.client?.file?.id;
  if (name && !LOGGING.has(name)) LOGGING.set(name, descriptor.logging);
}
for (const joke of MANIFEST_JOKES) {
  if (!byId.has(joke.id)) throw new Error(`${joke.id} is not in Mojang's list`);
}

// Templates for the archived old versions: Mojang's versions started by
// launchwrapper, oldest first (RubyDung has its own main class).
const templates = mojangVersions
  .filter((v) => v.entry.releaseTime < MODERN_FROM && !v.entry.id.startsWith('rd-'))
  .sort((a, b) => a.entry.releaseTime.localeCompare(b.entry.releaseTime));
function nearestOlder(time) {
  let found = templates[0];
  for (const template of templates) {
    if (template.entry.releaseTime <= time) found = template;
  }
  return found;
}

const archive = await json(ARCHIVE);
const candidates = archive.versions.filter(
  (v) => !mojangIds.has(v.id.toLowerCase()) && !['alpha_server', 'classic_server'].includes(v.type),
);
const fetched = await each(candidates, async (entry) => ({
  entry,
  descriptor: await json(entry.url),
}));

const extras = [];
const seenIds = new Set(mojangIds);
for (const { entry, descriptor: source } of fetched) {
  const client = source.downloads?.client;
  if (!client || mojangClients.has(client.sha1)) continue;
  const renamed = RENAMED[entry.id] ?? {};
  const kind = renamed.group ?? group(entry.id);
  const id = renamed.id ?? entry.id;
  if (!/^[A-Za-z0-9._ -]{1,64}$/.test(id) || seenIds.has(id.toLowerCase())) {
    throw new Error(`${entry.id}: unusable or repeated id ${id}`);
  }
  seenIds.add(id.toLowerCase());

  const downloads = { client: source.downloads.client };
  if (source.downloads.server) downloads.server = source.downloads.server;

  const templateId =
    renamed.template ?? FIRST_BUILDS[entry.id] ?? (source.arguments?.game ? null : undefined);
  let descriptor;
  let type;
  if (templateId === null) {
    // A modern build with a complete version file of its own.
    descriptor = { ...source, id, downloads };
    type = /^\d+\.\d+(\.\d+)?_unobfuscated$/.test(entry.id) ? 'release' : 'snapshot';
  } else {
    const template =
      templateId === undefined ? nearestOlder(entry.releaseTime) : byId.get(templateId);
    if (!template) throw new Error(`${entry.id}: no template ${templateId}`);
    const base = structuredClone(template.descriptor);
    descriptor = { ...base, id, downloads };
    type = template.entry.type;
  }
  descriptor.type = type;
  descriptor.time = entry.time ?? entry.releaseTime;
  descriptor.releaseTime = entry.releaseTime;
  delete descriptor.inheritsFrom;

  extras.push({
    id,
    name: renamed.name ?? name(entry.id, kind),
    ...(renamed.title ? { title: renamed.title } : {}),
    group: kind,
    ...(renamed.variant || kind === 'first-build' ? { variant: 'first-build' } : {}),
    type,
    releaseTime: entry.releaseTime,
    archiveId: entry.id,
    ...published(entry.id, descriptor),
  });
}

const jokes = [
  ...MANIFEST_JOKES.map((joke) => {
    const { entry, descriptor } = byId.get(joke.id);
    const downloads = { client: descriptor.downloads.client };
    if (descriptor.downloads.server) downloads.server = descriptor.downloads.server;
    return {
      id: joke.id,
      name: joke.name ?? joke.id,
      title: joke.title,
      group: 'joke',
      type: entry.type,
      releaseTime: entry.releaseTime,
      inMojangList: true,
      ...published(joke.id, { ...structuredClone(descriptor), downloads }),
    };
  }),
  ...extras.filter((v) => v.group === 'joke'),
];
const newestFirst = (a, b) => b.releaseTime.localeCompare(a.releaseTime);
jokes.sort(newestFirst);
const rest = extras.filter((v) => v.group !== 'joke').sort(newestFirst);

const document = {
  schemaVersion: 1,
  about: 'Generated by scripts/extra-versions.mjs (docs/EXTRA-VERSIONS.md). Do not edit by hand.',
  versions: [...jokes, ...rest],
};
writeFileSync(OUT, `${JSON.stringify(document, null, 1)}\n`);
const worlds = document.versions.filter(
  (v) => v.server && Date.parse(v.releaseTime) >= WORLDS_FROM,
);
writeFileSync(WORLDS_OUT, worlds.map((v) => `${v.id}\n`).join(''));
const counts = {};
for (const version of document.versions) counts[version.group] = (counts[version.group] ?? 0) + 1;
console.log(`${document.versions.length} versions`, counts);
