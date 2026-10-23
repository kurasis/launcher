// When a message was written, as the chat list and the messages say it, in the
// interface language. Times are Unix seconds, as the friends server sends them.
import { numberLocale } from '../../lib/strings';

const DAY_MS = 86_400_000;

/** Calendar days from `then` to `now` on this computer's clock (0 is the same day). */
function daysBetween(then: Date, now: Date): number {
  const day = (date: Date) =>
    Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
  return Math.round(day(now) - day(then));
}

/** A time the server sent that no date can hold (out of range, not a number) shows nothing. */
function dateOf(seconds: number): Date | null {
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** The machine-readable form of a message's time for `<time dateTime>`, if it has one. */
export function isoTime(seconds: number): string | undefined {
  return dateOf(seconds)?.toISOString();
}

function clock(date: Date): string {
  return date.toLocaleTimeString(numberLocale(), { hour: '2-digit', minute: '2-digit' });
}

/** The chat list: "19:41" today, "yesterday", then "12 Oct" (with the year when not this one). */
export function listTime(seconds: number, now = Date.now()): string {
  const date = dateOf(seconds);
  if (date === null) return '';
  const today = new Date(now);
  const days = daysBetween(date, today);
  if (days <= 0) return clock(date);
  if (days === 1) {
    return new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' }).format(-1, 'day');
  }
  return date.toLocaleDateString(numberLocale(), {
    day: 'numeric',
    month: 'short',
    ...(date.getFullYear() === today.getFullYear() ? {} : { year: 'numeric' }),
  });
}

/** Beside a message: "19:41" today, else "12 Oct, 19:41" (with the year when not this one). */
export function messageTime(seconds: number, now = Date.now()): string {
  const date = dateOf(seconds);
  if (date === null) return '';
  const today = new Date(now);
  if (daysBetween(date, today) <= 0) return clock(date);
  return date.toLocaleString(numberLocale(), {
    day: 'numeric',
    month: 'short',
    ...(date.getFullYear() === today.getFullYear() ? {} : { year: 'numeric' }),
    hour: '2-digit',
    minute: '2-digit',
  });
}
