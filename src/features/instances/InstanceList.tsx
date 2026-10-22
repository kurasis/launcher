import { useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { InstanceTile } from '../../components/InstanceTile';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';
import { healthTone } from './healthTone';
import {
  FIND_THRESHOLD,
  INSTANCE_SORTS,
  type InstanceSort,
  orderInstances,
  QUERY_MAX_LENGTH,
} from './instanceOrder';
import styles from './Instances.module.css';
import { gameLabel } from './loaderLabel';

interface InstanceListProps {
  instances: InstanceSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** Choosing is off (the settings cannot be saved); finding and ordering still work. */
  disabled: boolean;
  /** The heading that names the list. */
  labelledBy?: string | undefined;
}

const t = strings.instances;

/**
 * The installations as rows; the chosen one is marked. A long list gets a search
 * field and an order to choose (concept A, "many installations").
 */
export function InstanceList({
  instances,
  selectedId,
  onSelect,
  disabled,
  labelledBy,
}: InstanceListProps) {
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState<InstanceSort>('recent');
  if (instances.length === 0) {
    return <p className={styles.empty}>{t.empty}</p>;
  }
  const finding = instances.length > FIND_THRESHOLD;
  const shown = finding
    ? orderInstances(instances, {
        query,
        sort,
        locale: numberLocale(),
        loaderName: (kind) => strings.loaders.names[kind],
      })
    : instances;

  return (
    <div className={styles.listArea}>
      {finding && (
        <div className={styles.finder}>
          <div className={styles.search}>
            <Icon name="search" size={18} className={styles.searchIcon} />
            <input
              type="search"
              className={[forms.input, styles.searchInput].join(' ')}
              aria-label={t.find}
              placeholder={t.find}
              value={query}
              maxLength={QUERY_MAX_LENGTH}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => {
                setQuery(event.target.value.slice(0, QUERY_MAX_LENGTH));
              }}
            />
          </div>
          <div className={styles.sorts} role="group" aria-label={t.sortBy}>
            {INSTANCE_SORTS.map((option) => (
              <button
                key={option}
                type="button"
                className={styles.pill}
                aria-pressed={sort === option}
                onClick={() => {
                  setSort(option);
                }}
              >
                {t.sorts[option]}
              </button>
            ))}
          </div>
        </div>
      )}
      {shown.length === 0 ? (
        <div className={styles.nothing}>
          <p role="status">{t.nothingFound}</p>
          <Button
            variant="ghost"
            onClick={() => {
              setQuery('');
            }}
          >
            {t.clearSearch}
          </Button>
        </div>
      ) : (
        <ul className={styles.list} aria-labelledby={labelledBy}>
          {shown.map((instance) => {
            const selected = instance.id === selectedId;
            return (
              <li key={instance.id}>
                <button
                  type="button"
                  className={styles.row}
                  aria-current={selected ? 'true' : undefined}
                  disabled={disabled}
                  onClick={() => {
                    onSelect(instance.id);
                  }}
                >
                  <InstanceTile id={instance.id} size={40} />
                  <span className={styles.rowText}>
                    <span className={styles.rowName}>{instance.displayName}</span>
                    <bdi className={styles.rowMeta}>{gameLabel(instance)}</bdi>
                  </span>
                  <StatusBadge
                    className={styles.rowState}
                    tone={healthTone[instance.health]}
                    label={instance.busy ? t.busy : t.health[instance.health]}
                  />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
