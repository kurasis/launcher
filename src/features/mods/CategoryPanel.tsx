import { useId, useState } from 'react';
import { Icon } from '../../components/Icon';
import type { ModsKind } from '../../lib/contracts';
import { numberLocale, strings } from '../../lib/strings';
import { CATEGORIES, MAX_CATEGORIES, categoryName } from './catalogCategories';
import styles from './Catalog.module.css';

interface Props {
  kind: ModsKind;
  /** Projects in each category for the search, or `null` while unknown. */
  counts: ReadonlyMap<string, number> | null;
  /** Projects the search finds without categories, or `null` while unknown. */
  total: number | null;
  selected: readonly string[];
  /** What the counts are for, such as "Fabric 1.21.1". */
  scope: string | null;
  onToggle: (id: string) => void;
  onAll: () => void;
}

const count = (value: number) => value.toLocaleString(numberLocale());

/**
 * The categories of a kind (D-072, from variant 2): Modrinth's and CurseForge's in
 * one list with icons and counts, at the inline end of the catalog. Several can be
 * chosen; "All categories" clears them. Narrow, the list folds behind a button.
 */
export function CategoryPanel({ kind, counts, total, selected, scope, onToggle, onAll }: Props) {
  const t = strings.mods.catalog;
  const titleId = useId();
  const listId = useId();
  const [open, setOpen] = useState(false);
  const full = selected.length >= MAX_CATEGORIES;
  return (
    <aside className={styles.side} aria-labelledby={titleId} data-open={open}>
      <div className={styles.sideHead}>
        <h3 id={titleId} className={styles.sideTitle}>
          {t.categories}
        </h3>
        {scope !== null && (
          <span className={styles.sideHint}>{t.categoriesFor({ what: scope })}</span>
        )}
      </div>
      <button
        type="button"
        className={[styles.smallButton, styles.sideToggle].join(' ')}
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => {
          setOpen((now) => !now);
        }}
      >
        <span>
          {t.categories}
          {selected.length > 0 && ` · ${t.categoriesChosen({ count: selected.length })}`}
        </span>
        <Icon name={open ? 'up' : 'down'} size={16} />
      </button>
      <ul id={listId} className={styles.categoryList}>
        <li>
          <button
            type="button"
            className={styles.category}
            aria-pressed={selected.length === 0}
            onClick={onAll}
          >
            <Icon name="grid" size={18} />
            <span className={styles.categoryName}>{t.allCategories}</span>
            {total !== null && <span className={styles.count}>{count(total)}</span>}
          </button>
        </li>
        {CATEGORIES[kind].map((category) => {
          const pressed = selected.includes(category.id);
          const found = counts?.get(category.id) ?? null;
          return (
            <li key={category.id}>
              <button
                type="button"
                className={styles.category}
                aria-pressed={pressed}
                // Nothing to find there, or no room for one more: only a chosen one can go.
                disabled={!pressed && (found === 0 || full)}
                onClick={() => {
                  onToggle(category.id);
                }}
              >
                <Icon
                  name={category.icon}
                  size={18}
                  className={category.mirror === true ? styles.mirror : undefined}
                />
                <span className={styles.categoryName}>{categoryName(category.id)}</span>
                {found !== null && <span className={styles.count}>{count(found)}</span>}
              </button>
            </li>
          );
        })}
      </ul>
      <p className={styles.sideNote} role="status">
        {full ? t.categoriesFull({ count: MAX_CATEGORIES }) : t.categoriesNote}
      </p>
    </aside>
  );
}
