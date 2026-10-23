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

function clock(date: Date): string {
  return date.toLocaleTimeString(numberLocale(), { hour: '2-digit', minute: '2-digit' });
}

/** The chat list: "19:41" today, "yesterday", then "12 Oct" (with the year when not this one). */
export function listTime(seconds: number, now = Date.now()): string {
  const date = new Date(seconds * 1000);
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
  const date = new Date(seconds * 1000);
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
