import { useId, useMemo, useState } from 'react';
import forms from '../../components/forms.module.css';
import type { CatalogView, VersionType, VersionView } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './VersionPicker.module.css';

const t = strings.createForm;
const TYPES: VersionType[] = ['release', 'snapshot', 'old_beta', 'old_alpha'];

interface VersionPickerProps {
  catalog: CatalogView;
  value: string;
  onChange: (version: VersionView) => void;
  /** Only these versions, when a mod loader limits them (D-039). */
  allowed?: ReadonlySet<string> | null;
}

/** Every Mojang version, one tab per version type (D-035). */
export function VersionPicker({ catalog, value, onChange, allowed = null }: VersionPickerProps) {
  const versions = useMemo(
    () => (allowed === null ? catalog.versions : catalog.versions.filter((v) => allowed.has(v.id))),
    [catalog.versions, allowed],
  );
  const selected = versions.find((v) => v.id === value);
  const [type, setType] = useState<VersionType>(selected?.versionType ?? 'release');
  const [query, setQuery] = useState('');
  const searchId = useId();
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
  const needle = query.trim().toLowerCase();
  const shown = versions.filter(
    (v) => v.versionType === type && (needle === '' || v.id.toLowerCase().includes(needle)),
  );
  const latest = type === 'snapshot' ? catalog.latestSnapshot : catalog.latestRelease;

  return (
    <div className={styles.picker}>
      <div role="tablist" aria-label={t.version} className={styles.tabs}>
        {TYPES.map((item) => (
          <button
            key={item}
            type="button"
            role="tab"
            aria-selected={type === item}
            className={styles.tab}
            onClick={() => {
              setType(item);
            }}
          >
            {t.versionTypes[item]}
            <span className={styles.count}>{counts[item]}</span>
          </button>
        ))}
      </div>
      <p className={styles.hint}>{t.typeHints[type]}</p>
      <label htmlFor={searchId} className={styles.searchLabel}>
        {t.search}
      </label>
      <input
        id={searchId}
        type="search"
        className={forms.input}
        value={query}
        placeholder={t.search}
        autoComplete="off"
        onChange={(event) => {
          setQuery(event.target.value);
        }}
      />
      <ul className={styles.list} aria-label={t.versionTypes[type]}>
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
              {version.id === latest && <span className={styles.tag}>{t.latest}</span>}
              {version.verified && (
                <span className={[styles.tag, styles.verified].join(' ')}>{t.verified}</span>
              )}
              <span className={styles.date}>{version.releaseTime?.slice(0, 10) ?? ''}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
