import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Icon } from '../../components/Icon';
import forms from '../../components/forms.module.css';
import type { CatalogView, VersionType, VersionView } from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';
import styles from './VersionPicker.module.css';

const t = strings.createForm;
const TYPES: VersionType[] = ['release', 'snapshot', 'old_beta', 'old_alpha'];
/** Version ids are short; a longer search cannot match anything. */
const SEARCH_MAX_LENGTH = 32;

interface VersionPickerProps {
  catalog: CatalogView;
  value: string;
  onChange: (version: VersionView) => void;
  /** Only these versions, when a mod loader limits them (D-039). */
  allowed?: ReadonlySet<string> | null;
  /** Said before the version type's hint, such as which loader limits the list. */
  note?: string | null;
}

/** "8 Aug 2024" in the interface language; nothing for a missing or broken date. */
function releaseDate(releaseTime: string | null): string {
  if (releaseTime === null) return '';
  const date = new Date(releaseTime);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString(numberLocale(), { dateStyle: 'medium' });
}

/**
 * Every Mojang version, one tab per version type that has any (D-035). The tabs
 * follow the tabs pattern: one tab stop, arrow keys (mirrored right to left), Home
 * and End. The tab follows the selection when it moves to another type.
 */
export function VersionPicker({
  catalog,
  value,
  onChange,
  allowed = null,
  note = null,
}: VersionPickerProps) {
  const versions = useMemo(
    () => (allowed === null ? catalog.versions : catalog.versions.filter((v) => allowed.has(v.id))),
    [catalog.versions, allowed],
  );
  const selected = versions.find((v) => v.id === value);
  // The tab the player chose, and for which selection: a selection that moves
  // elsewhere (another loader, the catalog refreshed) takes the tab with it.
  const [picked, setPicked] = useState<{ type: VersionType; value: string } | null>(null);
  const [query, setQuery] = useState('');
  const baseId = useId();
  const searchId = `${baseId}-search`;
  const panelId = `${baseId}-panel`;
  const tabId = (type: VersionType) => `${baseId}-tab-${type}`;
  const listRef = useRef<HTMLUListElement>(null);

  const counts = useMemo(() => {
    const result: Record<VersionType, number> = {
      release: 0,
      snapshot: 0,
      old_beta: 0,
      old_alpha: 0,
    };
    for (const version of versions) result[version.versionType] += 1;
    return result;
  }, [versions]);
  const visible = TYPES.filter((type) => counts[type] > 0);
  const wanted = picked?.value === value ? picked.type : (selected?.versionType ?? 'release');
  const type = visible.includes(wanted) ? wanted : (selected?.versionType ?? visible[0] ?? wanted);
  const tabbed = visible.length > 1;

  const needle = query.trim().toLowerCase();
  const shown = versions.filter(
    (v) => v.versionType === type && (needle === '' || v.id.toLowerCase().includes(needle)),
  );
  const latest = type === 'snapshot' ? catalog.latestSnapshot : catalog.latestRelease;

  // The chosen version stays in sight inside the list, without scrolling the page.
  useEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>('[aria-pressed="true"]');
    if (list === null || row === null || row === undefined) return;
    const top = row.offsetTop;
    const bottom = top + row.offsetHeight;
    if (top < list.scrollTop || bottom > list.scrollTop + list.clientHeight) {
      list.scrollTop = Math.max(0, top - (list.clientHeight - row.offsetHeight) / 2);
    }
  }, [value, type]);

  const choose = (next: VersionType) => {
    setPicked({ type: next, value });
  };

  const onTabKey = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const forward = document.documentElement.dir === 'rtl' ? -1 : 1;
    let next: number;
    switch (event.key) {
      case 'ArrowRight':
        next = index + forward;
        break;
      case 'ArrowLeft':
        next = index - forward;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = visible.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    const target = visible[(next + visible.length) % visible.length];
    if (target === undefined) return;
    choose(target);
    document.getElementById(tabId(target))?.focus();
  };

  return (
    <div className={styles.picker}>
      <div className={styles.bar}>
        {tabbed && (
          <div role="tablist" aria-label={t.typesLabel} className={styles.tabs}>
            {visible.map((item, index) => (
              <button
                key={item}
                id={tabId(item)}
                type="button"
                role="tab"
                aria-selected={type === item}
                aria-controls={panelId}
                tabIndex={type === item ? 0 : -1}
                className={styles.tab}
                onClick={() => {
                  choose(item);
                }}
                onKeyDown={(event) => {
                  onTabKey(event, index);
                }}
              >
                {t.versionTypes[item]}
                <span className={styles.count}>{counts[item]}</span>
              </button>
            ))}
          </div>
        )}
        <div className={styles.search}>
          <label htmlFor={searchId} className={styles.searchLabel}>
            {t.search}
          </label>
          <Icon name="search" size={16} className={styles.searchIcon} />
          <input
            id={searchId}
            type="search"
            className={[forms.input, styles.searchInput].join(' ')}
            value={query}
            placeholder={t.search}
            maxLength={SEARCH_MAX_LENGTH}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => {
              setQuery(event.target.value.slice(0, SEARCH_MAX_LENGTH));
            }}
          />
        </div>
      </div>
      <div
        id={panelId}
        className={styles.panel}
        role={tabbed ? 'tabpanel' : undefined}
        aria-labelledby={tabbed ? tabId(type) : undefined}
      >
        <p className={styles.hint}>
          {note !== null && (
            <>
              <span>{note}</span>{' '}
            </>
          )}
          <span>{t.typeHints[type]}</span>
        </p>
        <ul ref={listRef} className={styles.list} aria-label={t.versionTypes[type]}>
          {shown.length === 0 && <li className={styles.empty}>{t.noMatch}</li>}
          {shown.map((version) => (
            <li key={version.id}>
              <button
                type="button"
                className={styles.row}
                aria-pressed={version.id === value}
                onClick={() => {
                  onChange(version);
                }}
              >
                <span className={styles.id}>{version.id}</span>
                {version.id === latest && (
                  <span className={[styles.tag, styles.latest].join(' ')}>{t.latest}</span>
                )}
                {version.verified && <span className={styles.tag}>{t.verified}</span>}
                {version.releaseTime !== null && (
                  <time className={styles.date} dateTime={version.releaseTime}>
                    {releaseDate(version.releaseTime)}
                  </time>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
