import { numberLocale, strings } from './strings';

const DAY_MS = 86_400_000;

/** Calendar days from `at` to `now` in local time: 0 today, 1 yesterday; never below 0. */
export function calendarDays(at: number, now = Date.now()): number {
  const day = (time: number) => {
    const date = new Date(time);
    return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
  };
  return Math.max(0, Math.round(day(now) - day(at)));
}

/** "today", "yesterday", "3 days ago": by calendar day, in the interface language. */
export function daysAgo(at: number, now = Date.now()): string {
  return new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' }).format(
    -calendarDays(at, now),
    'day',
  );
}

/**
 * A date said briefly: "today, 19:55" or "yesterday, 19:55" for the last two days, "3 October"
 * before, with the year when it is not this one; '' when it does not parse. `time: 'always'`
 * keeps the time on older days too (one day can hold several backups); `capital` is for a date
 * that starts its line ("Today, 19:55").
 */
export function shortWhen(
  at: string,
  { time = 'recent', capital = false }: { time?: 'recent' | 'always'; capital?: boolean } = {},
  now = Date.now(),
): string {
  const moment = Date.parse(at);
  if (Number.isNaN(moment)) return '';
  const date = new Date(moment);
  const clock = { hour: 'numeric', minute: '2-digit' } as const;
  if (calendarDays(moment, now) <= 1) {
    const day = daysAgo(moment, now);
    return strings.dates.dayTime({
      day: capital ? day.charAt(0).toLocaleUpperCase(numberLocale()) + day.slice(1) : day,
      time: new Intl.DateTimeFormat(numberLocale(), clock).format(date),
    });
  }
  return new Intl.DateTimeFormat(numberLocale(), {
    ...(date.getFullYear() === new Date(now).getFullYear() ? {} : { year: 'numeric' as const }),
    month: 'long',
    day: 'numeric',
    ...(time === 'always' ? clock : {}),
  }).format(date);
}
