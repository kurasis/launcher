# Translating the launcher

The interface reads every text from a catalog per language (D-038). English is the
source; any other language can be added without changing code.

## Files

- `src/locales/messages/en.json`: the source. `ui` holds the interface texts,
  `errors` the messages of the Rust core keyed by their English template. The
  `errors` section is generated from the Rust sources; do not edit it by hand.
- `src/locales/messages/<tag>.json`: one file per language, named by its BCP 47 tag
  (`de`, `pt-BR`, `zh-Hant`). Same keys as English; any subset may be translated.
- `src/locales/languages.json` and `src/lib/i18n/messages.ts`: generated.

## Adding a language

1. Copy `en.json` to `<tag>.json` and set `_meta`:
   ```json
   "_meta": { "name": "Deutsch", "dir": "ltr" }
   ```
   `name` is the language's name in itself. Use `"dir": "rtl"` for right-to-left
   scripts. `"numbering": "latn"` (optional) picks the digits numbers are written
   with.
2. Translate the values. Leave out what you have not translated: missing texts come
   from the region's base language (`pt-BR` → `pt`) and then from English.
3. Run `npm run i18n`, then `npm run i18n:check`. It lists any problem with the
   catalog, and the new language appears in Settings → General → Language with its
   translated share.

## Message format

Texts are [ICU MessageFormat](https://unicode-org.github.io/icu/userguide/format_parse/messages/):

- values: `Selected: {id}`; keep the names in braces as they are;
- plurals use your language's categories (`zero`, `one`, `two`, `few`, `many`,
  `other`) and `#` for the number:
  `{count, plural, one {# file} few {# files} many {# files} other {# files}}`;
- choices: `{kind, select, repair {Repair} other {Installation}}`;
- numbers: `{min, number, ::group-off}` writes the number without thousands
  separators;
- a literal `'` before `{` or `}` must be written `''`.

The check rejects a translation that uses a value the English text does not have.

## Right-to-left languages

In a right-to-left language the page direction flips. Paths and the console journal
stay left-to-right. When a text mixes left-to-right values separated only by
punctuation, wrap each in Unicode isolates (U+2066 … U+2069) so they keep their
order; see `details.windowValue` in `ar.json`.

## For developers

- New interface text: add it to `en.json` `ui`, run `npm run i18n`; the typed
  `strings` object then has it. A text with values is a function of them:
  `strings.details.memoryHint({ min, max, recommended })`.
- New core message with values: use `LauncherError::templated` (or
  `StorageIssue::templated`) with `{name}` placeholders instead of `format!`; the
  check fails on formatted messages because they cannot be translated.
