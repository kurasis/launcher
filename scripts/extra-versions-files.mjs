#!/usr/bin/env node
// Downloads the game jars of data/extra-versions.json from where they came
// from (Mojang or the archives), each checked against its SHA-1 and size, into
// <dir>/<sha1>.jar for the virus check and upload (the Versions workflow,
// D-073). Prints numbers only.
//
//   node scripts/extra-versions-files.mjs <dir> [--skip <file with one SHA-1 per line>]
//
// A jar already in <dir> with the right SHA-1 is kept. A jar whose SHA-1 is in
// the skip file (what upd.lapi.sh has already) is not downloaded.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const DATA = new URL('../data/extra-versions.json', import.meta.url);
const AT_ONCE = 4;

const [dir, ...rest] = process.argv.slice(2);
if (!dir) throw new Error('usage: extra-versions-files.mjs <dir> [--skip <file>]');
const skipFile = rest[0] === '--skip' ? rest[1] : null;
const skip = new Set(
  skipFile
    ? readFileSync(skipFile, 'utf8')
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => /^[0-9a-f]{40}$/.test(line))
    : [],
);

const sha1 = (bytes) => createHash('sha1').update(bytes).digest('hex');
const data = JSON.parse(readFileSync(DATA, 'utf8'));
const files = new Map();
for (const version of data.versions) {
  for (const file of version.files) files.set(file.sha1, file);
}

async function download(file) {
  const target = join(dir, `${file.sha1}.jar`);
  if (existsSync(target) && sha1(readFileSync(target)) === file.sha1) return 'kept';
  for (let attempt = 1; attempt <= 4; attempt += 1) {
    try {
      const response = await fetch(file.origin, { signal: AbortSignal.timeout(600_000) });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const bytes = Buffer.from(await response.arrayBuffer());
      if (bytes.length !== file.size || sha1(bytes) !== file.sha1) return 'mismatch';
      writeFileSync(`${target}.part`, bytes);
      renameSync(`${target}.part`, target);
      return 'fetched';
    } catch {
      rmSync(`${target}.part`, { force: true });
      await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
    }
  }
  return 'failed';
}

mkdirSync(dir, { recursive: true });
const wanted = [...files.values()].filter((file) => !skip.has(file.sha1));
const counts = { jars: files.size, skipped: files.size - wanted.length };
let next = 0;
await Promise.all(
  Array.from({ length: AT_ONCE }, async () => {
    while (next < wanted.length) {
      const file = wanted[next];
      next += 1;
      const result = await download(file);
      counts[result] = (counts[result] ?? 0) + 1;
    }
  }),
);
console.log(JSON.stringify(counts));
if (counts.mismatch || counts.failed) process.exit(1);
