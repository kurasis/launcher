// Interface translations (D-038): checks every catalog and generates what the app
// derives from them.
//
//   node scripts/i18n.mjs          update the generated files
//   node scripts/i18n.mjs --check  fail if anything is invalid or out of date
//
// Catalogs live in src/locales/messages/<tag>.json; English is the source. Each
// has `_meta` (its own name and writing direction), `ui` (nested ICU messages) and
// `errors` (core messages keyed by their English template, D-038). This script
//   - copies every user-facing message template from the Rust sources into the
//     English `errors` section, so translators see them like any other text;
//   - checks that every message parses as ICU MessageFormat, that no catalog has
//     keys English lacks, and that a translation uses only its source's values;
//   - writes src/lib/i18n/messages.ts (the typed shape of `ui`) and
//     src/locales/languages.json (available languages and how complete they are).
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, TYPE } from '@formatjs/icu-messageformat-parser';
import prettier from 'prettier';

const root = fileURLToPath(new URL('..', import.meta.url));
const messagesDir = join(root, 'src/locales/messages');
const manifestPath = join(root, 'src/locales/languages.json');
const typesPath = join(root, 'src/lib/i18n/messages.ts');
const rustDir = join(root, 'src-tauri/src');
const check = process.argv.includes('--check');
const problems = [];

// ---------------------------------------------------------------- Rust sources

function rustFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === 'tests' ? [] : rustFiles(path);
    return entry.name.endsWith('.rs') && entry.name !== 'tests.rs' ? [path] : [];
  });
}

/** A Rust string literal's value; `\` + newline skips the next line's indent. */
function rustString(raw) {
  return raw
    .replace(/\\\n\s*/g, '')
    .replace(/\\"/g, '"')
    .replace(/\\'/g, "'")
    .replace(/\\n/g, '\n')
    .replace(/\\\\/g, '\\');
}

const LITERAL = String.raw`"((?:[^"\\]|\\.|\\\n)*)"`;
/** Calls whose first string literal is a user-facing text. */
const SOURCES = [
  String.raw`LauncherError::(?:new|invalid_input|internal)\(\s*(?:ErrorCode::\w+,\s*)?`,
  String.raw`LauncherError::templated\(\s*(?:ErrorCode::\w+|\w+),\s*`,
  String.raw`StorageIssue::templated\(\s*ErrorCode::\w+,\s*`,
  // `(ErrorCode::X, "text")` pairs, such as the file-error templates.
  String.raw`\(\s*ErrorCode::\w+,\s*`,
  String.raw`\bmessage:\s*`,
  // Texts passed as values into other messages: file-operation contexts and
  // the names of things that failed to download.
  String.raw`from_io\(\s*&?\w+,\s*`,
  String.raw`status_error\(\s*\w+,\s*`,
  String.raw`\.fetch\(\s*[^,()]+,\s*[^,()]+,\s*`,
  String.raw`hash_mismatch\(\s*`,
].map((prefix) => new RegExp(prefix + LITERAL, 'g'));
const FORMATTED =
  /LauncherError::(?:new|invalid_input|internal)\(\s*(?:ErrorCode::\w+,\s*)?format!/g;

function rustTemplates() {
  const found = new Set();
  for (const file of rustFiles(rustDir)) {
    // Test modules come last in a file; their texts are not user-facing.
    const source = readFileSync(file, 'utf8').split(
      /#\[cfg\(test\)\]\s*(?:#\[cfg\([^)]*\)\]\s*)*(?:pub(?:\(crate\))? )?mod /,
    )[0];
    for (const pattern of SOURCES) {
      for (const match of source.matchAll(pattern)) found.add(rustString(match[1]));
    }
    if (FORMATTED.test(source)) {
      problems.push(
        `${relative(root, file)}: a formatted error message; use LauncherError::templated so it can be translated`,
      );
    }
    FORMATTED.lastIndex = 0;
  }
  return [...found].sort();
}

// ---------------------------------------------------------------- catalogs

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** `[path, message]` for every text in a tree. */
function leaves(tree, prefix = []) {
  return Object.entries(tree).flatMap(([key, value]) =>
    typeof value === 'string' ? [[[...prefix, key], value]] : leaves(value, [...prefix, key]),
  );
}

/** Names and kinds of the values a message uses. */
function argumentsOf(elements, out = new Map()) {
  for (const element of elements) {
    switch (element.type) {
      case TYPE.argument:
        if (!out.has(element.value)) out.set(element.value, 'string | number');
        break;
      case TYPE.number:
        out.set(element.value, 'number');
        break;
      case TYPE.date:
      case TYPE.time:
        out.set(element.value, 'Date | number');
        break;
      case TYPE.plural:
        out.set(element.value, 'number');
        for (const option of Object.values(element.options)) argumentsOf(option.value, out);
        break;
      case TYPE.select:
        out.set(element.value, 'string');
        for (const option of Object.values(element.options)) argumentsOf(option.value, out);
        break;
      case TYPE.tag:
        argumentsOf(element.children, out);
        break;
      default:
        break;
    }
  }
  return out;
}

function parsed(tag, path, message) {
  try {
    return argumentsOf(parse(message));
  } catch (error) {
    problems.push(`${tag}: ${path}: not valid ICU MessageFormat (${error.message})`);
    return new Map();
  }
}

const tags = readdirSync(messagesDir)
  .filter((name) => name.endsWith('.json'))
  .map((name) => name.slice(0, -'.json'.length))
  .sort((a, b) => (a === 'en' ? -1 : b === 'en' ? 1 : a.localeCompare(b)));
const catalogs = Object.fromEntries(
  tags.map((tag) => [tag, readJson(join(messagesDir, `${tag}.json`))]),
);
const en = catalogs.en;
if (en === undefined) throw new Error('src/locales/messages/en.json is missing');

// English errors are exactly the Rust templates, each its own source text.
const templates = rustTemplates();
const englishErrors = Object.fromEntries(templates.map((t) => [t, t]));
const enPath = join(messagesDir, 'en.json');
const enUpdated = `${JSON.stringify({ ...en, errors: englishErrors }, null, 2)}\n`;

const sourceUi = new Map(leaves(en.ui).map(([path, message]) => [path.join('.'), message]));
const sourceArgs = new Map(
  [...sourceUi].map(([path, message]) => [path, parsed('en', path, message)]),
);
const sourceErrors = new Map(
  Object.keys(englishErrors).map((key) => [key, parsed('en', `errors: ${key}`, key)]),
);

const manifest = [];
for (const tag of tags) {
  const catalog = catalogs[tag];
  const meta = catalog._meta;
  if (typeof meta?.name !== 'string' || !['ltr', 'rtl'].includes(meta?.dir)) {
    problems.push(`${tag}: _meta needs "name" (in the language itself) and "dir" ("ltr" or "rtl")`);
  }
  let translated = 0;
  for (const [path, message] of leaves(catalog.ui ?? {})) {
    const key = path.join('.');
    const source = sourceArgs.get(key);
    if (source === undefined) {
      problems.push(`${tag}: ui.${key} does not exist in English`);
      continue;
    }
    for (const name of parsed(tag, `ui.${key}`, message).keys()) {
      if (!source.has(name))
        problems.push(`${tag}: ui.${key} uses {${name}}, which English does not have`);
    }
    translated += 1;
  }
  for (const [key, message] of Object.entries(
    tag === 'en' ? englishErrors : (catalog.errors ?? {}),
  )) {
    const source = sourceErrors.get(key);
    if (source === undefined) {
      problems.push(`${tag}: errors["${key}"] is no longer a core message`);
      continue;
    }
    for (const name of parsed(tag, `errors: ${key}`, message).keys()) {
      if (!source.has(name))
        problems.push(`${tag}: errors["${key}"] uses {${name}}, which English does not have`);
    }
    translated += 1;
  }
  const total = sourceUi.size + sourceErrors.size;
  manifest.push({
    tag,
    name: meta?.name ?? tag,
    dir: meta?.dir ?? 'ltr',
    translated: Math.floor((translated / total) * 100),
  });
}

// ---------------------------------------------------------------- generated files

function typeOf(tree, indent) {
  const pad = '  '.repeat(indent);
  const lines = Object.entries(tree).map(([key, value]) => {
    const name = /^[A-Za-z_$][\w$]*$/.test(key) ? key : JSON.stringify(key);
    if (typeof value !== 'string') return `${pad}  readonly ${name}: ${typeOf(value, indent + 1)};`;
    const args = argumentsOf(parse(value));
    if (args.size === 0) return `${pad}  readonly ${name}: string;`;
    const fields = [...args].map(([arg, type]) => `${arg}: ${type}`).join('; ');
    return `${pad}  readonly ${name}: (values: { ${fields} }) => string;`;
  });
  return `{\n${lines.join('\n')}\n${pad}}`;
}

const typesText = `// Generated by scripts/i18n.mjs from src/locales/messages/en.json. Do not edit;
// run \`npm run i18n\` after changing the English catalog.

/** The interface texts: a string, or a function of the values the text shows. */
export type Messages = ${typeOf(en.ui, 0)};
`;
const manifestText = `${JSON.stringify(manifest, null, 2)}\n`;

const outputs = [
  [enPath, enUpdated],
  [typesPath, typesText],
  [manifestPath, manifestText],
];
for (const [path, raw] of outputs) {
  // Written exactly as the repository's formatter would leave them.
  const text = await prettier.format(raw, {
    ...(await prettier.resolveConfig(path)),
    filepath: path,
  });
  let current = null;
  try {
    current = readFileSync(path, 'utf8');
  } catch {
    current = null;
  }
  if (current === text) continue;
  if (check) problems.push(`${relative(root, path)} is out of date; run npm run i18n`);
  else writeFileSync(path, text);
}

if (problems.length > 0) {
  console.error(problems.map((p) => `i18n: ${p}`).join('\n'));
  process.exit(1);
}
console.log(
  manifest.map((l) => `${l.tag} ${l.name}: ${String(l.translated)}% (${l.dir})`).join('\n'),
);
