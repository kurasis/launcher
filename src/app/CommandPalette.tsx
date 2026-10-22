import { Fragment, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from '../components/Icon';
import { InstanceTile } from '../components/InstanceTile';
import type { AppSnapshot, ContentItem, InstanceSummary, ModsHit } from '../lib/contracts';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { type MainAction, mainActionOf } from '../features/instances/mainAction';
import styles from './CommandPalette.module.css';
import {
  CAPS,
  CATALOG_MIN_QUERY,
  MAX_QUERY,
  type PaletteEntry,
  type PaletteTarget,
  type Range,
  type Ranked,
  catalogEntries,
  highlightParts,
  holdsMods,
  installationEntries,
  installationOrder,
  installedModEntries,
  isEmptyQuery,
  matchText,
  normalizeQuery,
  pageEntries,
  rankEntries,
} from './palette';

/** The catalog is asked this long after the last key, not on every key. */
const CATALOG_DELAY_MS = 250;

/** Installations whose mods folders are read at the same time. */
const CONTENT_READERS = 3;

/** How soon after an IME composition ends a key still belongs to it, in ms. */
const COMPOSITION_ENTER_MS = 300;

interface CommandPaletteProps {
  snapshot: AppSnapshot | null;
  /** Command (⌘) instead of Ctrl in the key hints and for the main action. */
  macos: boolean;
  onClose: () => void;
  /**
   * Carries out a chosen row; `main` is Ctrl+Enter (⌘ Enter) on an installation:
   * its main action rather than opening it.
   */
  onChoose: (target: PaletteTarget, main: boolean) => void;
}

interface Group {
  key: string;
  heading: string;
  rows: Ranked[];
  /** A line under the rows: the catalog is offline or still searching. */
  note: string | null;
}

/** Key caps, left to right in every language. */
function Keys({ keys, className }: { keys: readonly string[]; className?: string | undefined }) {
  return (
    <span className={[styles.keys, className].filter(Boolean).join(' ')} dir="ltr">
      {keys.map((key) => (
        <kbd key={key} className={styles.key}>
          {key}
        </kbd>
      ))}
    </span>
  );
}

/** `text` with its matched parts in `<mark>`, built from nodes (never HTML). */
function Marked({ text, ranges }: { text: string; ranges: readonly Range[] }) {
  if (ranges.length === 0) return text;
  return highlightParts(text, ranges).map((part, index) =>
    part.mark ? <mark key={index}>{part.text}</mark> : <Fragment key={index}>{part.text}</Fragment>,
  );
}

function Lead({ entry }: { entry: PaletteEntry }) {
  const lead = entry.lead;
  if (lead.kind === 'tile') return <InstanceTile id={lead.id} size={32} />;
  if (lead.kind === 'icon') {
    return (
      <span className={styles.square} aria-hidden="true">
        <Icon name={lead.icon} size={18} />
      </span>
    );
  }
  if (lead.url !== null) {
    return (
      <img
        className={styles.picture}
        src={lead.url}
        alt=""
        loading="lazy"
        draggable={false}
        referrerPolicy="no-referrer"
      />
    );
  }
  return (
    <span className={styles.letter} aria-hidden="true">
      {lead.letter === '' ? <Icon name="mods" size={18} /> : lead.letter}
    </span>
  );
}

/**
 * The command palette (D-072, "Find · Ctrl K"): one field that finds installations,
 * their mods, the catalog's mods, screens, settings and actions. Focus stays in the
 * field; the arrows move through the rows (aria-activedescendant), Enter opens one,
 * Ctrl+Enter (⌘ Enter) runs an installation's main action, Escape closes.
 */
export function CommandPalette({ snapshot, macos, onClose, onChoose }: CommandPaletteProps) {
  const t = strings.palette;
  const baseId = useId();
  const listId = `${baseId}-list`;
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  // When the last IME composition ended. WebKit sends the Enter that ends one just
  // after compositionend, without isComposing: an Enter (or arrow, or Escape) that
  // soon, before a key is released, belongs to the composition. A candidate picked
  // with the mouse releases no key, so the mark also lapses after a moment.
  const composedAt = useRef<number | null>(null);
  const [raw, setRaw] = useState('');
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const query = normalizeQuery(raw);
  const empty = isEmptyQuery(query);
  const offline = snapshot?.network === 'offline';
  const mod = macos ? '⌘' : 'Ctrl';

  // Focus goes into the field, and back where it was when the palette closes; to
  // the screen when that is gone.
  useEffect(() => {
    const opener = document.activeElement;
    inputRef.current?.focus();
    return () => {
      if (!(opener instanceof HTMLElement) || opener === document.body) return;
      if (opener.isConnected) opener.focus();
      else document.querySelector<HTMLElement>('main')?.focus();
    };
  }, []);

  // Installed mods: the mods folders of installations with a mod loader, read once
  // per opening, a few at a time, the chosen installation first. Errors leave one out.
  const [modded] = useState<InstanceSummary[]>(() =>
    snapshot === null ? [] : installationOrder(snapshot).filter(holdsMods),
  );
  const [content, setContent] = useState<ReadonlyMap<string, readonly ContentItem[]>>(
    () => new Map(),
  );
  useEffect(() => {
    let alive = true;
    const waiting = modded.map((instance) => instance.id);
    const reader = async () => {
      for (let id = waiting.shift(); id !== undefined && alive; id = waiting.shift()) {
        const instanceId = id;
        try {
          const view = await ipc.modsContent(instanceId);
          const mods = view.items.filter((item) => item.folder === 'mods');
          // After the palette closed this changes nothing: its state is gone.
          setContent((previous) => new Map(previous).set(instanceId, mods));
        } catch {
          // An installation that cannot be read is left out of the results.
        }
      }
    };
    for (let i = 0; i < CONTENT_READERS; i += 1) void reader();
    return () => {
      alive = false;
    };
  }, [modded]);

  // The catalog: asked after a pause in typing, from two characters on, never
  // offline; an answer to an older query is dropped. The text is never logged.
  const catalogText =
    snapshot !== null && !offline && !empty && Array.from(query).length >= CATALOG_MIN_QUERY
      ? query
      : null;
  const [catalog, setCatalog] = useState<{ text: string; hits: ModsHit[] } | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    generation.current += 1;
    const asked = generation.current;
    if (catalogText === null) return;
    const timer = window.setTimeout(() => {
      ipc
        .modsSearch({
          text: catalogText,
          kind: 'mod',
          game: null,
          loader: null,
          category: null,
          sort: null,
          offset: 0,
          limit: CAPS.catalogMods,
        })
        .then(
          (page) => {
            if (generation.current === asked) {
              setCatalog({ text: catalogText, hits: page.hits.slice(0, CAPS.catalogMods) });
            }
          },
          () => {
            if (generation.current === asked) setCatalog({ text: catalogText, hits: [] });
          },
        );
    }, CATALOG_DELAY_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [catalogText]);
  const hits = catalogText !== null && catalog?.text === catalogText ? catalog.hits : null;

  // The rows: installations, mods (installed, then the catalog), pages and actions.
  const instances = snapshot === null ? [] : installationOrder(snapshot);
  const mainActions = new Map<string, MainAction>();
  if (snapshot !== null) {
    for (const instance of instances) {
      mainActions.set(instance.id, mainActionOf(instance, snapshot));
    }
  }
  const installationRows = rankEntries(
    installationEntries(instances, (instance) => {
      const main = mainActions.get(instance.id);
      if (main === undefined || main.kind === 'none') return t.open;
      return main.kind === 'running' ? strings.activity.launch : main.label;
    }),
    query,
    CAPS.installations,
  );
  // Built once per folder read and ranked once per query, never again for the arrows
  // or the pointer: there can be thousands of installed mods.
  const installedEntries = useMemo(() => installedModEntries(modded, content), [modded, content]);
  const installedRows = useMemo(
    () => (empty ? [] : rankEntries(installedEntries, query, CAPS.installedMods)),
    [installedEntries, query, empty],
  );
  const shownProjects = new Set(
    installedRows.flatMap(({ entry }) =>
      entry.target.kind === 'installedMod' && entry.target.projectId !== null
        ? [entry.target.projectId]
        : [],
    ),
  );
  const catalogRows: Ranked[] =
    hits === null
      ? []
      : catalogEntries(hits, shownProjects)
          .slice(0, CAPS.catalogMods)
          .map((entry) => {
            const found = matchText(entry.label, query);
            return {
              entry,
              match: { rank: 0, label: found === null ? [] : [found.range], detail: [] },
            };
          });
  const pageRows = rankEntries(pageEntries(snapshot?.settings), query, CAPS.pages);
  const modsNote = empty
    ? null
    : offline && Array.from(query).length >= CATALOG_MIN_QUERY
      ? t.offline
      : catalogText !== null && hits === null
        ? t.searching
        : null;
  const groups: Group[] = [
    { key: 'installations', heading: strings.instances.title, rows: installationRows, note: null },
    {
      key: 'mods',
      heading: strings.mods.title,
      rows: [...installedRows, ...catalogRows],
      note: modsNote,
    },
    { key: 'pages', heading: t.pages, rows: pageRows, note: null },
  ].filter((group) => group.rows.length > 0 || group.note !== null);
  const options = groups.flatMap((group) => group.rows);
  // Where each group's rows start in the one list the arrows move through.
  const starts: number[] = [];
  let total = 0;
  for (const group of groups) {
    starts.push(total);
    total += group.rows.length;
  }
  const active = options.find((row) => row.entry.key === activeKey) ?? options[0];
  const activeIndex = active === undefined ? -1 : options.indexOf(active);
  const optionId = (index: number) => `${baseId}-option-${String(index)}`;
  const activeId = activeIndex === -1 ? undefined : optionId(activeIndex);

  // The active row stays in view (jsdom has no scrolling).
  useEffect(() => {
    const row = listRef.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (row !== null && row !== undefined && typeof row.scrollIntoView === 'function') {
      row.scrollIntoView({ block: 'nearest' });
    }
  }, [activeId]);

  const choose = (entry: PaletteEntry, main: boolean) => {
    onChoose(entry.target, main && entry.target.kind === 'installation');
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const composing =
      event.nativeEvent.isComposing ||
      (composedAt.current !== null && event.timeStamp - composedAt.current < COMPOSITION_ENTER_MS);
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        if (composing) return;
        event.preventDefault();
        if (options.length === 0) return;
        const step = event.key === 'ArrowDown' ? 1 : -1;
        const next = options[(activeIndex + step + options.length) % options.length];
        if (next !== undefined) setActiveKey(next.entry.key);
        return;
      }
      case 'Enter': {
        if (composing) {
          composedAt.current = null;
          return;
        }
        event.preventDefault();
        if (active !== undefined) choose(active.entry, macos ? event.metaKey : event.ctrlKey);
        return;
      }
      case 'Escape':
        // Escape listeners of the screen behind (dialogs, the gallery) never see it.
        event.stopPropagation();
        // An IME uses Escape to drop what is being composed.
        if (composing) return;
        event.preventDefault();
        onClose();
        return;
      case 'Tab':
        // The field is the only stop: focus stays in the palette.
        event.preventDefault();
        return;
      default:
        return;
    }
  };

  const count = t.results({ count: options.length });

  return createPortal(
    <div
      className={styles.backdrop}
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        // The browser would move focus to what lies under the pointer once the
        // palette is gone, over the focus it gave back.
        event.preventDefault();
        onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t.title}
        className={styles.dialog}
        onMouseDown={(event) => {
          // A press on a heading, a hint or the space between rows keeps focus in the
          // field, where the keys work.
          if (event.target !== inputRef.current) event.preventDefault();
        }}
      >
        <div className={styles.field}>
          <Icon name="search" size={20} className={styles.searchIcon} />
          <input
            ref={inputRef}
            className={styles.input}
            type="text"
            role="combobox"
            aria-label={t.title}
            aria-expanded={options.length > 0}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={activeId}
            autoComplete="off"
            spellCheck={false}
            maxLength={MAX_QUERY}
            dir="auto"
            placeholder={t.placeholder}
            value={raw}
            onChange={(event) => {
              setRaw(event.target.value);
              setActiveKey(null);
            }}
            onKeyDown={onKeyDown}
            onKeyUp={() => {
              composedAt.current = null;
            }}
            onCompositionEnd={(event) => {
              composedAt.current = event.timeStamp;
            }}
          />
          <Keys keys={['Esc']} className={styles.escape} />
        </div>

        <div className={styles.results}>
          <div ref={listRef} role="listbox" id={listId} aria-label={t.listLabel}>
            {groups.map((group, groupIndex) => {
              const headingId = `${baseId}-${group.key}`;
              return (
                <div
                  key={group.key}
                  role="group"
                  aria-labelledby={headingId}
                  className={styles.group}
                >
                  <div role="presentation" id={headingId} className={styles.heading}>
                    {group.heading}
                  </div>
                  {group.rows.map(({ entry, match }, rowIndex) => {
                    const index = (starts[groupIndex] ?? 0) + rowIndex;
                    const isActive = index === activeIndex;
                    const main =
                      entry.target.kind === 'installation'
                        ? mainActions.get(entry.target.instanceId)
                        : undefined;
                    const status =
                      main !== undefined && (main.kind === 'running' || main.kind === 'busy');
                    const id = optionId(index);
                    const isConsole = entry.target.kind === 'console';
                    return (
                      <div
                        key={entry.key}
                        id={id}
                        role="option"
                        aria-selected={isActive}
                        // Name, second line and action, read with pauses between them.
                        aria-labelledby={
                          isConsole
                            ? `${id}-name ${id}-detail`
                            : `${id}-name ${id}-detail ${id}-action`
                        }
                        className={styles.option}
                        onMouseDown={(event) => {
                          // Focus stays in the field.
                          event.preventDefault();
                        }}
                        onMouseMove={() => {
                          if (!isActive) setActiveKey(entry.key);
                        }}
                        onClick={(event) => {
                          choose(entry, macos ? event.metaKey : event.ctrlKey);
                        }}
                      >
                        <Lead entry={entry} />
                        <span className={styles.text}>
                          <span className={styles.name} id={`${id}-name`}>
                            <bdi>
                              <Marked text={entry.label} ranges={match.label} />
                            </bdi>
                          </span>
                          <span className={styles.detail} id={`${id}-detail`}>
                            <Marked text={entry.detail} ranges={match.detail} />
                          </span>
                        </span>
                        <span className={styles.end}>
                          {isConsole ? (
                            <Keys keys={[mod, 'Shift', 'L']} />
                          ) : (
                            <span
                              id={`${id}-action`}
                              className={[styles.action, status && styles.status]
                                .filter(Boolean)
                                .join(' ')}
                            >
                              {entry.action}
                            </span>
                          )}
                          {/* The key for what the end of the row says: an installation's
                              main action is Ctrl+Enter (Enter opens it), the rest Enter. */}
                          {isActive && main === undefined && <Keys keys={['Enter']} />}
                          {isActive && main?.enabled === true && <Keys keys={[mod, 'Enter']} />}
                        </span>
                      </div>
                    );
                  })}
                  {group.note !== null && (
                    <div role="presentation" className={styles.note}>
                      {group.note}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
          {options.length === 0 && modsNote !== t.searching && (
            <p className={styles.empty}>{t.nothing}</p>
          )}
        </div>

        <p role="status" className={styles.visuallyHidden}>
          {modsNote === null ? count : `${count}. ${modsNote}`}
        </p>

        <div className={styles.footer}>
          <span className={styles.hint}>
            <Keys keys={['↑', '↓']} />
            {t.hints.move}
          </span>
          <span className={styles.hint}>
            <Keys keys={['Enter']} />
            {t.hints.open}
          </span>
          <span className={styles.hint}>
            <Keys keys={[mod, 'Enter']} />
            {t.hints.main}
          </span>
          <span className={styles.hint}>
            <Keys keys={['Esc']} />
            {t.hints.close}
          </span>
          <span className={styles.scope}>{t.scope}</span>
        </div>
      </div>
    </div>,
    document.body,
  );
}
