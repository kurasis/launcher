#!/usr/bin/env node
// Writes release metadata next to built packages (docs/spec/07 "Build and CI"):
// SHA256SUMS and build-info.json with app version, commit, target, toolchain
// versions and signing status. Usage:
//   node scripts/package-metadata.mjs --target <name> --out <dir> --signing <status> <file>...
// Only hashes and describes files; it never uploads or publishes anything.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

function parseArgs(argv) {
  const options = { files: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--target' || arg === '--out' || arg === '--signing') {
      options[arg.slice(2)] = argv[(i += 1)];
    } else {
      options.files.push(arg);
    }
  }
  if (!options.target || !options.out || !options.signing || options.files.length === 0) {
    throw new Error(
      'usage: package-metadata.mjs --target <name> --out <dir> --signing <status> <file>...',
    );
  }
  return options;
}

function run(command, args) {
  try {
    return execFileSync(command, args, { encoding: 'utf8' }).trim().split('\n')[0];
  } catch {
    return null;
  }
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

const options = parseArgs(process.argv.slice(2));
const root = new URL('..', import.meta.url);
const tauriConfig = JSON.parse(readFileSync(new URL('src-tauri/tauri.conf.json', root), 'utf8'));
const appVersion =
  tauriConfig.version ?? JSON.parse(readFileSync(new URL('package.json', root), 'utf8')).version;
const tauriCli = JSON.parse(
  readFileSync(new URL('node_modules/@tauri-apps/cli/package.json', root), 'utf8'),
).version;

mkdirSync(options.out, { recursive: true });
const artifacts = options.files.map((file) => {
  const name = basename(file);
  const target = join(options.out, name);
  copyFileSync(file, target);
  return { file: name, bytes: statSync(target).size, sha256: sha256(target) };
});

const info = {
  product: tauriConfig.productName,
  identifier: tauriConfig.identifier,
  appVersion,
  commit: process.env.GITHUB_SHA ?? run('git', ['rev-parse', 'HEAD']),
  // Local builds from uncommitted changes are marked; CI builds a clean checkout.
  dirtyWorkingTree: process.env.GITHUB_SHA ? false : run('git', ['status', '--porcelain']) !== '',
  target: options.target,
  builtAt: new Date().toISOString(),
  signing: options.signing,
  microsoftClientIdConfigured: (process.env.KURASIS_MSA_CLIENT_ID ?? '').trim().length > 0,
  toolchain: {
    rustc: run('rustc', ['--version']),
    cargo: run('cargo', ['--version']),
    node: process.version,
    tauriCli,
    glibc: process.platform === 'linux' ? run('ldd', ['--version']) : null,
  },
  ci: process.env.GITHUB_RUN_ID
    ? { runId: process.env.GITHUB_RUN_ID, runner: process.env.RUNNER_OS ?? null }
    : null,
  artifacts,
};

writeFileSync(
  join(options.out, 'SHA256SUMS'),
  artifacts.map((a) => `${a.sha256}  ${a.file}\n`).join(''),
);
writeFileSync(join(options.out, 'build-info.json'), `${JSON.stringify(info, null, 2)}\n`);
for (const artifact of artifacts) {
  console.log(`${artifact.sha256}  ${artifact.file}  (${artifact.bytes} bytes)`);
}
