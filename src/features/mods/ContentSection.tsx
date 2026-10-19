import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  ContentIdentified,
  ContentItem,
  ContentUpdate,
  ContentView,
  InstanceSummary,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { ourPicture } from './helpers';
import styles from './Mods.module.css';

const FOLDERS = ['mods', 'resourcepacks', 'shaderpacks', 'datapacks'] as const;

/** Where an update stands while "Update all" runs. */
interface Batch {
  done: number;
  total: number;
  failed: number;
}

interface Props {
  instance: InstanceSummary;
  onOpen: (projectId: string) => void;
  /** Opens the add dialog for a newer version that replaces this file. */
  onUpdate: (update: ContentUpdate, title: string, world: string | null) => void;
  /** Bumped by the parent after an install, to read the folder again. */
  revision: number;
}

function Row({
  item,
  update,
  known,
  busy,
  onToggle,
  onRemove,
  onOpen,
  onUpdate,
}: {
  item: ContentItem;
  update: ContentUpdate | undefined;
  known: ContentIdentified | undefined;
  busy: boolean;
  onToggle: () => void;
  onRemove: () => void;
  onOpen: (projectId: string) => void;
  onUpdate: () => void;
}) {
  const t = strings.mods.content;
  const [confirming, setConfirming] = useState(false);
  const icon = ourPicture(item.icon);
  const project = item.projectId ?? known?.projectId ?? null;
  return (
    <li className={styles.contentRow}>
      {icon !== null ? (
        <img className={styles.icon} src={icon} alt="" loading="lazy" />
      ) : (
        <span className={styles.icon} aria-hidden="true" />
      )}
      <span className={styles.cardBody}>
        <span className={styles.cardTitle}>
          <bdi>{item.title ?? item.name}</bdi>
          {item.versionNumber !== null && (
            <span className={styles.muted}>
              <bdi>{item.versionNumber}</bdi>
            </span>
          )}
          {!item.enabled && <StatusBadge tone="neutral" label={t.off} />}
          {item.dependency && <StatusBadge tone="neutral" label={t.dependency} />}
          {item.projectId === null && <StatusBadge tone="neutral" label={t.byHand} />}
          {update !== undefined && (
            <StatusBadge tone="warning" label={t.updateTo({ version: update.toNumber })} />
          )}
        </span>
        <span className={styles.muted}>
          <bdi dir="ltr">{item.path}</bdi>
          {!item.directory && ` · ${formatBytes(item.size)}`}
          {item.world !== null && ` · ${t.inWorld({ world: item.world })}`}
        </span>
      </span>
      <span className={styles.rowActions}>
        {!item.directory && (
          <label className={styles.check}>
            <input
              type="checkbox"
              checked={item.enabled}
              disabled={busy}
              onChange={onToggle}
              aria-label={t.toggle({ name: item.title ?? item.name })}
            />
            {t.on}
          </label>
        )}
        {update !== undefined && (
          <Button variant="primary" disabled={busy} onClick={onUpdate}>
            {t.update}
          </Button>
        )}
        {project !== null && (
          <Button
            onClick={() => {
              onOpen(project);
            }}
          >
            {t.open}
          </Button>
        )}
        {!item.directory &&
          (confirming ? (
            <>
              <Button
                variant="primary"
                disabled={busy}
                onClick={() => {
                  setConfirming(false);
                  onRemove();
                }}
              >
                {t.confirmRemove}
              </Button>
              <Button
                onClick={() => {
                  setConfirming(false);
                }}
              >
                {t.cancel}
              </Button>
            </>
          ) : (
            <Button
              disabled={busy}
              onClick={() => {
                setConfirming(true);
              }}
            >
              {t.remove}
            </Button>
          ))}
      </span>
    </li>
  );
}

/**
 * The content of an installation's game folder (D-066): turn files on or off,
 * remove them, and update what the catalog knows.
 */
export function ContentSection({ instance, onOpen, onUpdate, revision }: Props) {
  const t = strings.mods.content;
  const id = instance.id;
  const [view, setView] = useState<ContentView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  // Offers and recognized files for the folder as it was at `revision`.
  const [checked, setChecked] = useState<{
    revision: number;
    updates: ContentUpdate[];
    known: ContentIdentified[];
  } | null>(null);
  const updates = checked?.revision === revision ? checked.updates : null;
  const known = checked?.revision === revision ? checked.known : [];
  const [checking, setChecking] = useState(false);
  const [batch, setBatch] = useState<Batch | null>(null);

  const load = useCallback(() => {
    void ipc
      .modsContent(id)
      .then(setView)
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      });
  }, [id]);

  useEffect(load, [load, revision]);

  const change = (action: Promise<void>) => {
    setWorking(true);
    setError(null);
    action
      .then(() => {
        // Paths change when a file is turned off; check again for offers.
        setChecked(null);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setWorking(false);
        load();
      });
  };

  const check = () => {
    setChecking(true);
    setError(null);
    Promise.all([ipc.modsUpdates(id), ipc.modsIdentify(id)])
      .then(([offers, identified]) => {
        setChecked({ revision, updates: offers, known: identified });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setChecking(false);
      });
  };

  // One after another; one that cannot go (something missing or in the way)
  // is counted and left for the player to open.
  const updateAll = async (offers: ContentUpdate[]) => {
    setBatch({ done: 0, total: offers.length, failed: 0 });
    let failed = 0;
    for (const [index, offer] of offers.entries()) {
      const world = /^saves\/([^/]+)\/datapacks\//.exec(offer.path)?.[1] ?? null;
      const payload = {
        instanceId: id,
        version: offer.toVersionId,
        world,
        replaces: offer.path,
      };
      try {
        const plan = await ipc.modsPlan(payload);
        // A download a metered connection asks about is left for the player.
        const ready =
          plan.missing.length === 0 && plan.conflicts.length === 0 && plan.approval === null;
        if (ready) await ipc.modsInstall(payload);
        else failed += 1;
      } catch {
        failed += 1;
      }
      setBatch({ done: index + 1, total: offers.length, failed });
    }
    load();
    check();
  };

  const busy = instance.busy || working || (batch !== null && batch.done < batch.total);
  const items = view?.items ?? [];
  const byPath = new Map((updates ?? []).map((u) => [u.path, u]));
  const knownByPath = new Map(known.map((k) => [k.path, k]));
  return (
    <div className={styles.section}>
      {instance.busy && <Notice tone="info">{t.busy}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button disabled={checking || busy} onClick={check}>
          {checking ? t.checking : t.check}
        </Button>
        {updates !== null && updates.length > 1 && (
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => {
              void updateAll(updates);
            }}
          >
            {t.updateAll({ count: updates.length })}
          </Button>
        )}
        <Button
          onClick={() => {
            void ipc.openInstanceFolder(id, undefined, 'mods').catch(() => undefined);
          }}
        >
          {t.openFolder}
        </Button>
      </div>
      {updates !== null && updates.length === 0 && (
        <p className={styles.muted} role="status">
          {t.upToDate}
        </p>
      )}
      {batch !== null && (
        <p role="status" className={styles.muted}>
          {batch.done < batch.total
            ? t.updating({ done: batch.done, total: batch.total })
            : batch.failed > 0
              ? t.updatedSome({ failed: batch.failed })
              : t.updatedAll}
        </p>
      )}
      {view === null ? (
        error === null && <p role="status">{strings.mods.loading}</p>
      ) : items.length === 0 ? (
        <p className={styles.muted}>{t.empty}</p>
      ) : (
        FOLDERS.map((folder) => {
          const inFolder = items.filter((i) => i.folder === folder);
          if (inFolder.length === 0) return null;
          return (
            <section key={folder} aria-label={t.folders[folder]}>
              <h3 className={styles.subTitle}>
                {t.folders[folder]} <span className={styles.muted}>{inFolder.length}</span>
              </h3>
              <ul className={styles.contentList}>
                {inFolder.map((item) => (
                  <Row
                    key={item.path}
                    item={item}
                    update={byPath.get(item.path)}
                    known={knownByPath.get(item.path)}
                    busy={busy}
                    onOpen={onOpen}
                    onToggle={() => {
                      change(ipc.modsSetEnabled(id, item.path, !item.enabled));
                    }}
                    onRemove={() => {
                      change(ipc.modsRemove(id, item.path));
                    }}
                    onUpdate={() => {
                      const offer = byPath.get(item.path);
                      if (offer !== undefined) onUpdate(offer, item.title ?? item.name, item.world);
                    }}
                  />
                ))}
              </ul>
            </section>
          );
        })
      )}
    </div>
  );
}
