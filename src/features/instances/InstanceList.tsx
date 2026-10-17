import { StatusBadge } from '../../components/StatusBadge';
import type { InstanceSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { healthTone } from './healthTone';
import styles from './Instances.module.css';
import { gameLabel } from './loaderLabel';

interface InstanceListProps {
  instances: InstanceSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  disabled: boolean;
}

const t = strings.instances;
/** Tile colors cycle through the list so neighbouring installations differ. */
const tiles = [styles.tile0, styles.tile1, styles.tile2, styles.tile3];

export function InstanceList({ instances, selectedId, onSelect, disabled }: InstanceListProps) {
  if (instances.length === 0) {
    return <p className={styles.muted}>{t.empty}</p>;
  }
  return (
    <ul className={styles.list}>
      {instances.map((instance, index) => {
        const selected = instance.id === selectedId;
        return (
          <li key={instance.id}>
            <button
              type="button"
              className={[styles.item, selected ? styles.selected : ''].filter(Boolean).join(' ')}
              aria-current={selected ? 'true' : undefined}
              disabled={disabled}
              onClick={() => {
                onSelect(instance.id);
              }}
            >
              <span
                className={[styles.tile, tiles[index % tiles.length]].join(' ')}
                aria-hidden="true"
              />
              <span className={styles.itemName}>{instance.displayName}</span>
              <span className={styles.itemMeta}>
                <span>{gameLabel(instance)}</span>
                <StatusBadge
                  tone={healthTone[instance.health]}
                  label={instance.busy ? t.busy : t.health[instance.health]}
                />
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
