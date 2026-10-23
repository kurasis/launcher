import { afterEach, describe, expect, it } from 'vitest';
import type { RunSummary } from '../../lib/contracts';
import { loadLanguage, setLanguage } from '../../lib/strings';
import { instanceFixture } from '../../test/fixtures';
import { formatDuration, lastRunLine } from './lastRunLine';

const played = instanceFixture({
  lastPlayedAt: '2026-10-20T12:00:00Z',
  lastResult: {
    kind: 'launch',
    outcome: 'failed',
    finishedAt: '2026-10-20T13:05:00Z',
    exitCode: 1,
  },
});

const run: RunSummary = {
  runId: 'run-1',
  kind: 'game',
  instanceId: played.id,
  state: 'failed',
  startedAt: '2026-10-20T11:59:58Z',
  endedAt: '2026-10-20T13:05:00Z',
  exitCode: 1,
  durationMs: 65 * 60_000,
  drainTimedOut: false,
  outputTail: [],
  outputLinesDropped: 0,
  error: null,
  loggingDegraded: false,
  signal: null,
  diagnosis: null,
  server: null,
};

const now = new Date('2026-10-22T12:00:00Z');

/** The start as the interface writes it, in this computer's time zone. */
function when(locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    month: 'long',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(new Date('2026-10-20T12:00:00Z'));
}

describe('lastRunLine', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('says how long a game ran', () => {
    expect(formatDuration(20_000)).toBe('less than a minute');
    expect(formatDuration(60_000)).toBe('1 minute');
    expect(formatDuration(42 * 60_000)).toBe('42 minutes');
    expect(formatDuration(2 * 3_600_000)).toBe('2 hours');
    expect(formatDuration(65 * 60_000)).toBe('1 hour 5 minutes');
  });

  it('tells the start, the length and the ending of this installation’s last game', () => {
    expect(lastRunLine(played, run, now)).toBe(
      `Last played ${when('en')}, 1 hour 5 minutes. The game ended with an error.`,
    );
    // Another year shows the year.
    expect(lastRunLine(played, run, new Date('2027-01-02T00:00:00Z'))).toMatch(/2026/);
  });

  it('leaves the length out when the run is not that game', () => {
    for (const other of [
      { ...run, instanceId: 'another' },
      { ...run, kind: 'lifecycle_test' as const },
      { ...run, endedAt: '2026-10-19T10:00:00Z' },
      { ...run, durationMs: null },
      { ...run, durationMs: -5 },
      { ...run, durationMs: Number.NaN },
    ]) {
      const line = lastRunLine(played, other, now);
      expect(line).toMatch(/^Last played .+\. The game ended with an error\.$/);
      expect(line).not.toMatch(/hour|minute/);
    }
  });

  it('says nothing when the result is not about the last game', () => {
    expect(lastRunLine(instanceFixture(), run, now)).toBeNull();
    const repaired = instanceFixture({
      lastPlayedAt: played.lastPlayedAt,
      lastResult: { kind: 'repair', outcome: 'succeeded', finishedAt: '2026-10-21T09:00:00Z' },
    });
    expect(lastRunLine(repaired, run, now)).toBeNull();
    const runningAgain = { ...played, lastPlayedAt: '2026-10-22T11:00:00Z' };
    expect(lastRunLine(runningAgain, run, now)).toBeNull();
    const broken = { ...played, lastPlayedAt: 'yesterday' };
    expect(lastRunLine(broken, run, now)).toBeNull();
  });

  it('leaves the ending out when the launcher stopped first', () => {
    const interrupted = instanceFixture({
      lastPlayedAt: '2026-10-20T12:00:00Z',
      lastResult: { kind: 'launch', outcome: 'interrupted', finishedAt: '2026-10-20T13:05:00Z' },
    });
    expect(lastRunLine(interrupted, null, now)).toMatch(/^Last played [^.]+\.$/);
  });

  it('speaks Russian and Arabic', async () => {
    setLanguage(await loadLanguage('ru'));
    expect(formatDuration(42 * 60_000)).toBe('42 минуты');
    expect(lastRunLine(played, run, now)).toBe(
      `Последний запуск ${when('ru')}, 1 час 5 минут. Игра завершилась с ошибкой.`,
    );
    setLanguage(await loadLanguage('ar'));
    expect(formatDuration(2 * 60_000)).toBe('دقيقتين');
    expect(lastRunLine(played, run, now)).toBe(
      `آخر تشغيل يوم ${when('ar-u-nu-latn')}، لمدة ساعة واحدة و5 دقائق. انتهت اللعبة بخطأ.`,
    );
  });
});
