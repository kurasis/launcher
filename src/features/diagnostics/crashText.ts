import type { CrashKind, CrashNote } from '../../lib/contracts';
import { strings } from '../../lib/strings';

type Values = Record<string, string | number>;
type Message = string | ((values: Values) => string);
interface KindTexts {
  title: Message;
  cause: Message;
  fix: Message;
}

/** Details a message may name; an absent one reads "none", which messages select on. */
const KEYS = [
  'option',
  'memory',
  'suggested',
  'required',
  'found',
  'mod',
  'modId',
  'dependency',
  'dependencyId',
  'count',
  'files',
  'file',
  'loader',
  'place',
  'area',
  'error',
  'module',
  'vendor',
  'program',
  'signal',
  'library',
  'description',
  'exception',
  'entity',
  'block',
  'position',
  'seconds',
  'exitCode',
] as const;

/** Details formatted as numbers (MiB amounts and counts). */
const NUMBERS = new Set(['memory', 'suggested', 'count']);

function values(details: Record<string, string>): Values {
  const result: Values = {};
  for (const key of KEYS) result[key] = 'none';
  result.count = 1;
  for (const [key, value] of Object.entries(details)) {
    const number = Number(value);
    result[key] = NUMBERS.has(key) && Number.isFinite(number) ? number : value;
  }
  return result;
}

function say(message: Message, details: Record<string, string>): string {
  return typeof message === 'string' ? message : message(values(details));
}

function texts(kind: CrashKind): KindTexts {
  return strings.crash.kinds[kind] as unknown as KindTexts;
}

/** The short name of a cause, such as "The game ran out of memory". */
export function crashTitle(kind: CrashKind, details: Record<string, string> = {}): string {
  return say(texts(kind).title, details);
}

/** What happened and what to do, in the current language. */
export function crashText(note: CrashNote): { title: string; cause: string; fix: string } {
  const message = texts(note.kind);
  return {
    title: say(message.title, note.details),
    cause: say(message.cause, note.details),
    fix: say(message.fix, note.details),
  };
}
