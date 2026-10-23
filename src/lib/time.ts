import { numberLocale } from './strings';

const DAY_MS = 86_400_000;

/** "today", "yesterday", "3 days ago": by calendar day, in the interface language. */
export function daysAgo(at: number, now = Date.now()): string {
  const day = (time: number) => {
    const date = new Date(time);
    return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
  };
  const days = Math.max(0, Math.round(day(now) - day(at)));
  return new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' }).format(-days, 'day');
}
