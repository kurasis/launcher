import { useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Choice } from '../../components/Choice';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Segmented } from '../../components/Segmented';
import { StateDot } from '../../components/StateDot';
import forms from '../../components/forms.module.css';
import type { ConsoleEntry, ConsoleRun } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { direction, LANGUAGE_STORAGE_KEY, storedLanguage, strings } from '../../lib/strings';
import { applyTheme, storedTheme, THEME_STORAGE_KEY } from '../../lib/theme';
import { useLanguage } from '../../lib/useLanguage';
import { formatDuration } from '../instances/lastRunLine';
import styles from './ConsoleApp.module.css';

const t = strings.console;
/** How often the window asks the core for new lines. */
const POLL_MS = 300;
/** Lines kept in this window; the core keeps the same number. */
const KEEP = 10_000;
/** A full page from the core means more is waiting: ask again at once. */
const CORE_PAGE = 2_000;
/** The longest filter text: it is matched against every kept line on each change. */
const QUERY_MAX_LENGTH = 200;
const MINUTE = 60_000;
/** A longer run time says nothing true: the line leaves it out. */
const LONGEST_RUN_MS = 1000 * 60 * MINUTE;

type SourceFilter = 'all' | 'launcher' | 'game';
type LevelFilter = 'all' | 'warn' | 'error';

function time(at: string): string {
  const date = new Date(at);
  const pad = (n: number, width = 2) => String(n).padStart(width, '0');
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

/** A line as copied: the tag in capitals marks the source in plain text. */
function line(entry: ConsoleEntry): string {
  return `${time(entry.at)} ${t.sourceTag[entry.source]} ${entry.text}`;
}

/**
 * The status line's word on the game the launcher runs now (D-072): "Game running for
 * 42 minutes", in whole minutes so it never runs ahead of the clock.
 */
function gameLine(run: ConsoleRun | undefined): string | null {
  if (run === undefined) return null;
  switch (run.state) {
    case 'preflight':
    case 'spawning':
      return t.gameStarting;
    case 'running':
    case 'stopping': {
      const ms = run.runningMs;
      if (typeof ms !== 'number' || !Number.isFinite(ms) || ms < 0 || ms > LONGEST_RUN_MS) {
        return t.gameRunning;
      }
      return t.gameRunningFor({ duration: formatDuration(Math.floor(ms / MINUTE) * MINUTE) });
    }
    default:
      return null;
  }
}

/** The console window (D-037): launcher journal and game output, live. */
export function ConsoleApp() {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [missed, setMissed] = useState(0);
  const [offline, setOffline] = useState(false);
  // Kept as its words, so a read that changes nothing renders nothing.
  const [game, setGame] = useState<string | null>(null);
  const [source, setSource] = useState<SourceFilter>('all');
  const [level, setLevel] = useState<LevelFilter>('all');
  const [query, setQuery] = useState('');
  const [follow, setFollow] = useState(true);
  const [copied, setCopied] = useState<'no' | 'yes' | 'failed'>('no');
  const copyTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const list = useRef<HTMLDivElement>(null);
  // The console has no access to settings; it follows the language and the theme the
  // main window mirrors into local storage (D-038).
  const [wanted, setWanted] = useState(storedLanguage);
  const lang = useLanguage(wanted);
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === LANGUAGE_STORAGE_KEY) setWanted(storedLanguage());
      if (event.key === THEME_STORAGE_KEY) applyTheme(storedTheme());
    };
    window.addEventListener('storage', onStorage);
    // A theme kept after this window's first paint, before it listened.
    applyTheme(storedTheme());
    return () => {
      window.removeEventListener('storage', onStorage);
      clearTimeout(copyTimer.current);
    };
  }, []);
  useEffect(() => {
    try {
      void getCurrentWebviewWindow()
        .setTitle(t.windowTitle)
        .catch(() => undefined);
    } catch {
      // Outside the app (tests) there is no window to retitle.
    }
  }, [lang]);

  useEffect(() => {
    let stopped = false;
    let after = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      let delay = POLL_MS;
      try {
        const page = await ipc.readConsole(after);
        if (stopped) return;
        after = page.lastSeq;
        setOffline(false);
        setGame(gameLine(page.run));
        if (page.missed > 0) setMissed((m) => m + page.missed);
        if (page.entries.length > 0) {
          setEntries((previous) => {
            const next = previous.concat(page.entries);
            return next.length > KEEP ? next.slice(next.length - KEEP) : next;
          });
        }
        if (page.entries.length >= CORE_PAGE) delay = 0;
      } catch {
        if (!stopped) {
          setOffline(true);
          // Without an answer the window cannot know that the game still runs.
          setGame(null);
        }
      }
      if (!stopped) timer = setTimeout(() => void tick(), delay);
    };
    void tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, []);

  const needle = query.trim().toLowerCase();
  const shown = useMemo(
    () =>
      entries.filter(
        (e) =>
          (source === 'all' || e.source === source) &&
          (level === 'all' || (level === 'warn' ? e.level !== 'info' : e.level === 'error')) &&
          (needle === '' || e.text.toLowerCase().includes(needle)),
      ),
    [entries, source, level, needle],
  );

  // The status line and the notices change the log's height without a new line: the
  // newest line stays in view across those too.
  useEffect(() => {
    const element = list.current;
    if (follow && element !== null) element.scrollTop = element.scrollHeight;
  }, [shown, follow, game, offline, missed]);

  const copy = () => {
    void navigator.clipboard
      .writeText(shown.map(line).join('\n'))
      .then(
        () => {
          setCopied('yes');
        },
        () => {
          setCopied('failed');
        },
      )
      .finally(() => {
        // A second press shows its own answer for the whole time.
        clearTimeout(copyTimer.current);
        copyTimer.current = setTimeout(() => {
          setCopied('no');
        }, 1500);
      });
  };

  return (
    <div className={styles.shell}>
      <header className={styles.toolbar}>
        <h1 className={styles.title}>{t.title}</h1>
        <Segmented
          label={t.sourceLabel}
          options={(['all', 'launcher', 'game'] as const).map((item) => ({
            value: item,
            label: t.sources[item],
          }))}
          value={source}
          compact
          onChange={setSource}
        />
        <select
          aria-label={t.levelLabel}
          className={forms.select}
          value={level}
          onChange={(event) => {
            setLevel(event.target.value as LevelFilter);
          }}
        >
          {(['all', 'warn', 'error'] as const).map((item) => (
            <option key={item} value={item}>
              {t.levels[item]}
            </option>
          ))}
        </select>
        <div className={styles.search}>
          <Icon name="search" size={16} className={styles.searchIcon} />
          <input
            type="search"
            aria-label={t.search}
            placeholder={t.search}
            className={[forms.input, styles.searchInput].join(' ')}
            maxLength={QUERY_MAX_LENGTH}
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value.slice(0, QUERY_MAX_LENGTH));
            }}
          />
        </div>
        {/* What acts on the shown lines stays together at the end, on a row of its own
            in a narrow window. */}
        <div className={styles.actions}>
          <Choice
            type="checkbox"
            title={t.follow}
            checked={follow}
            onChange={(event) => {
              setFollow(event.target.checked);
            }}
          />
          <Button size="small" onClick={copy}>
            <Icon name="copy" size={18} />
            {copied === 'yes' ? t.copied : copied === 'failed' ? t.copyFailed : t.copy}
          </Button>
          <Button
            size="small"
            onClick={() => {
              setEntries([]);
              setMissed(0);
            }}
          >
            {t.clear}
          </Button>
        </div>
      </header>
      {(offline || missed > 0) && (
        <div className={styles.notices}>
          {offline && <Notice tone="danger">{t.unreachable}</Notice>}
          {/* The count can grow on every read: shown, not read out each time. */}
          {missed > 0 && (
            <Notice tone="warning" live={false}>
              {t.missed({ count: missed })}
            </Notice>
          )}
        </div>
      )}
      {/* Times, Java output and the core's lines read left to right in every language. */}
      <div
        ref={list}
        className={styles.lines}
        dir="ltr"
        role="log"
        aria-label={t.logLabel}
        aria-live="off"
        tabIndex={0}
        onScroll={(event) => {
          const element = event.currentTarget;
          const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
          if (atBottom !== follow) setFollow(atBottom);
        }}
      >
        {shown.length === 0 && (
          <p className={styles.empty} dir={direction()}>
            {entries.length === 0 ? t.empty : t.noMatch}
          </p>
        )}
        {shown.map((entry) => (
          <div
            key={entry.seq}
            className={styles.line}
            data-level={entry.level}
            data-source={entry.source}
            data-stderr={entry.stderr === true ? 'true' : undefined}
          >
            <span className={styles.time}>{time(entry.at)}</span>
            <span className={styles.tag}>{t.sources[entry.source]}</span>
            <span className={styles.text}>{entry.text}</span>
          </div>
        ))}
      </div>
      {/* Not a live region: the count changes on every read. */}
      <footer className={styles.status}>
        <span>{t.count({ shown: shown.length, total: entries.length })}</span>
        {game !== null && <StateDot tone="busy" label={game} />}
        <span className={styles.keeps}>{t.keeps({ count: KEEP })}</span>
      </footer>
    </div>
  );
}
