import type { InstanceSummary, RunSummary } from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';

const MINUTE = 60_000;

/** "42 minutes", "1 hour 5 minutes" or "less than a minute". */
export function formatDuration(ms: number): string {
  const t = strings.instances.lastRun;
  if (ms < MINUTE) return t.lessThanMinute;
  const minutes = Math.round(ms / MINUTE);
  if (minutes < 60) return t.minutes({ count: minutes });
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (rest === 0) return t.hours({ count: hours });
  return t.hoursMinutes({ hours: t.hours({ count: hours }), minutes: t.minutes({ count: rest }) });
}

function formatWhen(at: number, now: Date): string {
  const date = new Date(at);
  return new Intl.DateTimeFormat(numberLocale(), {
    // The year only when it is not this one.
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' as const }),
    month: 'long',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(date);
}

/**
 * How long the last game ran: only known for a game of this session (the snapshot keeps
 * one finished run), and only when that run is the one the installation's result is about.
 */
function durationOf(instance: InstanceSummary, lastRun: RunSummary | null, finished: number) {
  if (
    lastRun === null ||
    lastRun.kind !== 'game' ||
    lastRun.instanceId !== instance.id ||
    lastRun.durationMs === null ||
    lastRun.endedAt === null ||
    !Number.isFinite(lastRun.durationMs) ||
    lastRun.durationMs < 0
  ) {
    return null;
  }
  return Date.parse(lastRun.endedAt) === finished ? lastRun.durationMs : null;
}

/**
 * "Last played 20 October at 19:12, 42 minutes. The game ended without errors." Only
 * when the installation's last result is its last game: an install or a repair since
 * then replaces it, and while a game runs the result is the previous game's.
 */
export function lastRunLine(
  instance: InstanceSummary,
  lastRun: RunSummary | null,
  now: Date = new Date(),
): string | null {
  const result = instance.lastResult;
  if (result?.kind !== 'launch' || instance.lastPlayedAt === null) return null;
  const started = Date.parse(instance.lastPlayedAt);
  const finished = Date.parse(result.finishedAt);
  if (Number.isNaN(started) || Number.isNaN(finished) || finished < started) return null;
  const t = strings.instances.lastRun;
  const when = formatWhen(started, now);
  const duration = durationOf(instance, lastRun, finished);
  const played =
    duration === null
      ? t.played({ when })
      : t.playedFor({ when, duration: formatDuration(duration) });
  // "Interrupted" means the launcher stopped first: how the game ended is not known.
  if (result.outcome === 'interrupted') return played;
  return `${played} ${t.outcome[result.outcome]}`;
}
