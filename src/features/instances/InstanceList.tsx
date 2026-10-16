import { StatusBadge } from '../../components/StatusBadge';
import type { InstanceSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { healthTone } from './healthTone';
import styles from './Instances.module.css';

interface InstanceListProps {
  instances: InstanceSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  disabled: boolean;
}

const t = strings.instances;

export function InstanceList({ instances, selectedId, onSelect, disabled }: InstanceListProps) {
  if (instances.length === 0) {
    return <p className={styles.muted}>{t.empty}</p>;
  }
  return (
    <ul className={styles.list}>
      {instances.map((instance) => {
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
              <span className={styles.itemName}>{instance.displayName}</span>
              <span className={styles.itemMeta}>
                <span>Minecraft {instance.versionId}</span>
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
