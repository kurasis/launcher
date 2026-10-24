import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import type { WorldsImportable } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import { isolate } from '../mods/helpers';
import styles from './Worlds.module.css';

interface Props {
  /** The installations' single-player worlds; null while they are read. */
  sources: WorldsImportable[] | null;
  error: string | null;
  onImport: (instanceId: string, save: string) => void;
}

/** A save's bytes, when the core could count them. */
function sizeOf(source: WorldsImportable, index: number): number | null {
  const sizes: unknown = source.sizes;
  const bytes: unknown = Array.isArray(sizes) ? sizes[index] : null;
  return typeof bytes === 'number' && Number.isFinite(bytes) && bytes >= 0 ? bytes : null;
}

/**
 * The single-player worlds of this computer's installations, each with Import: a copy
 * goes into worlds with friends, the original stays where it is (W9).
 */
export function SinglePlayerWorlds({ sources, error, onImport }: Props) {
  const t = strings.worlds.local;
  const saves =
    sources?.flatMap((source) =>
      source.saves.map((save, index) => ({ source, save, size: sizeOf(source, index) })),
    ) ?? null;
  return (
    <Panel title={t.title} level={3}>
      <p className={styles.muted}>{t.hint}</p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {saves === null ? (
        error === null && <p className={styles.muted}>{strings.worlds.loading}</p>
      ) : saves.length === 0 ? (
        error === null && <p className={styles.muted}>{t.none}</p>
      ) : (
        <ul className={styles.saveList}>
          {saves.map(({ source, save, size }) => {
            const facts = {
              installation: isolate(source.instanceName),
              game: gameLabel({ versionId: source.minecraft, loader: source.loader }),
            };
            return (
              <li key={`${source.instanceId}/${save}`} className={styles.row}>
                <span className={styles.rowName} dir="auto">
                  {save}
                </span>
                <span className={styles.rowMeta}>
                  {size === null
                    ? t.meta(facts)
                    : t.metaSize({ ...facts, size: formatBytes(size) })}
                </span>
                <div className={styles.rowActions}>
                  <Button
                    size="small"
                    aria-label={t.importNamed({ name: isolate(save) })}
                    onClick={() => {
                      onImport(source.instanceId, save);
                    }}
                  >
                    {strings.worlds.import.submit}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}
