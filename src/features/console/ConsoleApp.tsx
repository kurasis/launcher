import { useEffect, useMemo, useRef, useState } from 'react';
import type { ConsoleEntry } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { LANGUAGE_STORAGE_KEY, storedLanguage, strings } from '../../lib/strings';
import { useLanguage } from '../../lib/useLanguage';
import styles from './ConsoleApp.module.css';

const t = strings.console;
/** How often the window asks the core for new lines. */
const POLL_MS = 300;
/** Lines kept in this window; the core keeps the same number. */
const KEEP = 10_000;
/** A full page from the core means more is waiting: ask again at once. */
const CORE_PAGE = 2_000;

type SourceFilter = 'all' | 'launcher' | 'game';
type LevelFilter = 'all' | 'warn' | 'error';

function time(at: string): string {
  const date = new Date(at);
  const pad = (n: number, width = 2) => String(n).padStart(width, '0');
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

function line(entry: ConsoleEntry): string {
  return `${time(entry.at)} ${t.sourceTag[entry.source]} ${entry.text}`;
}

/** The console window (D-037): launcher journal and game output, live. */
export function ConsoleApp() {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  const [missed, setMissed] = useState(0);
  const [offline, setOffline] = useState(false);
  const [source, setSource] = useState<SourceFilter>('all');
  const [level, setLevel] = useState<LevelFilter>('all');
  const [query, setQuery] = useState('');
  const [follow, setFollow] = useState(true);
  const [copied, setCopied] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  // The console has no access to settings; it follows the language the main
  // window mirrors into local storage (D-038).
  const [wanted, setWanted] = useState(storedLanguage);
  const lang = useLanguage(wanted);
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === LANGUAGE_STORAGE_KEY) setWanted(storedLanguage());
    };
    window.addEventListener('storage', onStorage);
    return () => {
      window.removeEventListener('storage', onStorage);
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
        if (page.missed > 0) setMissed((m) => m + page.missed);
        if (page.entries.length > 0) {
          setEntries((previous) => {
            const next = previous.concat(page.entries);
            return next.length > KEEP ? next.slice(next.length - KEEP) : next;
          });
        }
        if (page.entries.length >= CORE_PAGE) delay = 0;
      } catch {
        if (!stopped) setOffline(true);
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

  useEffect(() => {
    const element = list.current;
    if (follow && element !== null) element.scrollTop = element.scrollHeight;
  }, [shown, follow]);

  const copy = () => {
    void navigator.clipboard
      .writeText(shown.map(line).join('\n'))
      .then(() => {
        setCopied(true);
        setTimeout(() => {
          setCopied(false);
        }, 1500);
      })
      .catch(() => undefined);
  };

  return (
    <div className={styles.shell}>
      <header className={styles.toolbar}>
        <h1 className={styles.title}>{t.title}</h1>
        <div role="group" aria-label={t.sourceLabel} className={styles.segments}>
          {(['all', 'launcher', 'game'] as const).map((item) => (
            <button
              key={item}
              type="button"
              aria-pressed={source === item}
              className={styles.segment}
              onClick={() => {
                setSource(item);
              }}
            >
              {t.sources[item]}
            </button>
          ))}
        </div>
        <select
          aria-label={t.levelLabel}
          className={styles.control}
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
        <input
          type="search"
          aria-label={t.search}
          placeholder={t.search}
          className={[styles.control, styles.search].join(' ')}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
        />
        <label className={styles.follow}>
          <input
            type="checkbox"
            checked={follow}
            onChange={(event) => {
              setFollow(event.target.checked);
            }}
          />
          {t.follow}
        </label>
        <button type="button" className={styles.control} onClick={copy}>
          {copied ? t.copied : t.copy}
        </button>
        <button
          type="button"
          className={styles.control}
          onClick={() => {
            setEntries([]);
            setMissed(0);
          }}
        >
          {t.clear}
        </button>
      </header>
      {offline && (
        <p role="alert" className={styles.banner}>
          {t.unreachable}
        </p>
      )}
      {missed > 0 && <p className={styles.banner}>{t.missed({ count: missed })}</p>}
      <div
        ref={list}
        className={styles.lines}
        dir="ltr"
        role="log"
        aria-live="off"
        onScroll={(event) => {
          const element = event.currentTarget;
          const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
          if (atBottom !== follow) setFollow(atBottom);
        }}
      >
        {shown.length === 0 && (
          <p className={styles.empty}>{entries.length === 0 ? t.empty : t.noMatch}</p>
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
            <span className={styles.tag}>{t.sourceTag[entry.source]}</span>
            <span className={styles.text}>{entry.text}</span>
          </div>
        ))}
      </div>
      <footer className={styles.status}>
        {t.count({ shown: shown.length, total: entries.length })}
      </footer>
    </div>
  );
}
