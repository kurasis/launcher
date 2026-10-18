import { useCallback, useEffect, useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type {
  AppSnapshot,
  BackupInfo,
  BackupOverview,
  BackupScope,
  InstanceSummary,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { gameLabel } from '../instances/loaderLabel';
import styles from './Backups.module.css';

const t = strings.backups;

function when(at: string): string {
  return new Date(at).toLocaleString(numberLocale());
}

function scopeLabel(scope: BackupScope): string {
  return scope.kind === 'world' ? t.scopeWorld({ name: scope.name }) : t.scopeInstallation;
}

/** A confirmation waiting for the user: restore in place or delete. */
type Pending = { action: 'restore' | 'delete'; backup: BackupInfo } | null;

/** Backup settings: automatic backups after playing and how many to keep. */
function BackupSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const settings = snapshot.settings;
  const [keep, setKeep] = useState(String(settings.backupKeep));
  const update = useCommand(
    (requestId: string, patch: { backupAfterPlay?: boolean; backupKeep?: number }) =>
      ipc.setAppSettings(patch, requestId),
  );
  return (
    <div className={styles.settings}>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={settings.backupAfterPlay}
          disabled={update.pending || !settings.writable}
          onChange={(event) => void update.run({ backupAfterPlay: event.target.checked })}
        />
        <span>
          <span className={styles.strong}>{t.afterPlay}</span>
          <span className={styles.hint}>{t.afterPlayHint}</span>
        </span>
      </label>
      <form
        className={styles.keep}
        onSubmit={(event) => {
          event.preventDefault();
          void update.run({ backupKeep: Number(keep) });
        }}
      >
        <Field label={t.keep} hint={t.keepHint}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              type="number"
              min={1}
              max={100}
              value={keep}
              onChange={(event) => {
                setKeep(event.target.value);
              }}
            />
          )}
        </Field>
        <Button type="submit" disabled={update.pending || Number(keep) === settings.backupKeep}>
          {t.save}
        </Button>
      </form>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
    </div>
  );
}

/**
 * Incremental backups of worlds and whole installations (D-047): back up now,
 * restore in place or as a copy, delete, and see the space used.
 */
export function BackupsView({ snapshot }: { snapshot: AppSnapshot }) {
  const { instances } = snapshot;
  const selectId = useId();
  const [chosen, setChosen] = useState(snapshot.settings.selectedInstanceId);
  const [overview, setOverview] = useState<BackupOverview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending>(null);
  const [message, setMessage] = useState<string | null>(null);
  const instance: InstanceSummary | undefined =
    instances.find((i) => i.id === chosen) ?? instances[0];

  const reload = useCallback(() => {
    ipc
      .listBackups()
      .then((found) => {
        setOverview(found);
        setLoadError(null);
      })
      .catch((error: unknown) => {
        setLoadError(describeIpcError(error));
      });
  }, []);
  // Reload when an operation ends: automatic backups run after a game session.
  const activeId = snapshot.activeOperation?.operationId ?? null;
  useEffect(() => {
    if (activeId === null) reload();
  }, [activeId, reload]);

  const create = useCommand((requestId: string, id: string, scope: BackupScope) =>
    ipc.createBackup(id, scope, requestId),
  );
  const restore = useCommand((requestId: string, id: string, asCopy: boolean) =>
    ipc.restoreBackup(id, asCopy, requestId),
  );
  const remove = useCommand((requestId: string, id: string) => ipc.deleteBackup(id, requestId));
  const working = create.pending || restore.pending || remove.pending;
  const busy = snapshot.activeOperation !== null || working;
  const error = create.error ?? restore.error ?? remove.error;

  const after = (text: string) => {
    setMessage(text);
    reload();
  };

  // Backups of deleted installations (D-055).
  const orphaned = (overview?.backups ?? []).filter(
    (b) => !instances.some((i) => i.id === b.instanceId),
  );
  const deletedPanel = orphaned.length > 0 && (
    <DeletedInstallations
      backups={orphaned}
      busy={snapshot.activeOperation !== null}
      after={reload}
    />
  );

  if (instance === undefined) {
    return (
      <>
        <Panel title={t.title}>
          <p>{strings.settings.noInstallations}</p>
        </Panel>
        {deletedPanel}
      </>
    );
  }
  const worlds = overview?.worlds.find((w) => w.instanceId === instance.id)?.worlds ?? [];
  const backups = (overview?.backups ?? []).filter((b) => b.instanceId === instance.id);
  const lastOf = (name: string) =>
    backups.find((b) => b.scope.kind === 'world' && b.scope.name === name);

  return (
    <>
      <Panel
        title={t.title}
        aside={
          overview !== null && (
            <span className={styles.hint}>
              {t.storage({ size: formatBytes(overview.storeBytes) })}
            </span>
          )
        }
      >
        <p className={styles.hint}>{t.hint}</p>
        <BackupSettings key={snapshot.settings.backupKeep} snapshot={snapshot} />
      </Panel>
      <Panel title={t.worlds}>
        <div className={styles.field}>
          <label htmlFor={selectId}>{t.installation}</label>
          <select
            id={selectId}
            className={forms.select}
            value={instance.id}
            onChange={(event) => {
              setChosen(event.target.value);
              setPending(null);
              setMessage(null);
            }}
          >
            {instances.map((item) => (
              <option key={item.id} value={item.id}>
                {item.displayName} · {gameLabel(item)}
              </option>
            ))}
          </select>
        </div>
        {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
        {snapshot.activeOperation !== null && <Notice tone="info">{t.busy}</Notice>}
        {worlds.length === 0 ? (
          <p>{t.noWorlds}</p>
        ) : (
          <ul className={styles.list}>
            {worlds.map((name) => {
              const last = lastOf(name);
              return (
                <li key={name} className={styles.row}>
                  <div>
                    <strong>{name}</strong>
                    <span className={styles.hint}>
                      {last === undefined ? t.never : t.lastBackup({ date: when(last.createdAt) })}
                    </span>
                  </div>
                  <Button
                    disabled={busy}
                    onClick={() => {
                      void create.run(instance.id, { kind: 'world', name }).then((done) => {
                        if (done) after(t.created({ what: scopeLabel(done.scope) }));
                      });
                    }}
                  >
                    {t.backUp}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
        <div className={forms.actions}>
          <Button
            disabled={busy}
            onClick={() => {
              void create.run(instance.id, { kind: 'installation' }).then((done) => {
                if (done) after(t.created({ what: scopeLabel(done.scope) }));
              });
            }}
          >
            {t.whole}
          </Button>
        </div>
        <p className={styles.hint}>{t.wholeHint}</p>
      </Panel>
      <Panel title={t.list}>
        {working && <p role="status">{t.working}</p>}
        {message !== null && !working && <Notice tone="info">{message}</Notice>}
        {error !== null && <Notice tone="danger">{error}</Notice>}
        {backups.length === 0 ? (
          <p>{t.none}</p>
        ) : (
          <ul className={styles.list}>
            {backups.map((backup) => (
              <li key={backup.id} className={styles.row}>
                <div>
                  <strong>{scopeLabel(backup.scope)}</strong>
                  <span className={styles.tag}>{t.reason[backup.reason]}</span>
                  <span className={styles.hint}>
                    {when(backup.createdAt)} · {backup.versionLabel}
                  </span>
                  <span className={styles.hint}>
                    {t.size({
                      files: backup.fileCount,
                      total: formatBytes(backup.totalBytes),
                      added: formatBytes(backup.newBytes),
                    })}
                  </span>
                  {pending?.backup.id === backup.id && (
                    <Notice tone="warning">
                      <p>
                        {pending.action === 'restore'
                          ? t.confirmRestore({
                              what: scopeLabel(backup.scope),
                              date: when(backup.createdAt),
                            })
                          : t.confirmDelete({ date: when(backup.createdAt) })}
                      </p>
                      <div className={forms.actions}>
                        <Button
                          variant="primary"
                          disabled={busy}
                          onClick={() => {
                            setPending(null);
                            if (pending.action === 'restore') {
                              void restore.run(backup.id, false).then((done) => {
                                if (done) after(t.restored);
                              });
                            } else {
                              void remove.run(backup.id).then((freed) => {
                                if (freed !== undefined) {
                                  after(t.deleted({ size: formatBytes(freed) }));
                                }
                              });
                            }
                          }}
                        >
                          {pending.action === 'restore' ? t.restore : t.delete}
                        </Button>
                        <Button
                          onClick={() => {
                            setPending(null);
                          }}
                        >
                          {strings.createForm.cancel}
                        </Button>
                      </div>
                    </Notice>
                  )}
                </div>
                <div className={styles.actions}>
                  <Button
                    disabled={busy}
                    onClick={() => {
                      setPending({ action: 'restore', backup });
                    }}
                  >
                    {t.restore}
                  </Button>
                  {backup.scope.kind === 'world' && (
                    <Button
                      disabled={busy}
                      onClick={() => {
                        void restore.run(backup.id, true).then((done) => {
                          if (done?.world) after(t.restoredCopy({ name: done.world }));
                        });
                      }}
                    >
                      {t.restoreCopy}
                    </Button>
                  )}
                  <Button
                    disabled={busy}
                    onClick={() => {
                      setPending({ action: 'delete', backup });
                    }}
                  >
                    {t.delete}
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </Panel>
      {deletedPanel}
    </>
  );
}

/**
 * Backups whose installation was deleted (D-055): a backup of the whole
 * installation brings it back (not installed yet, with its game folder);
 * world backups can only be deleted.
 */
function DeletedInstallations({
  backups,
  busy,
  after,
}: {
  backups: BackupInfo[];
  busy: boolean;
  after: () => void;
}) {
  const [confirming, setConfirming] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const restore = useCommand((requestId: string, id: string) =>
    ipc.restoreBackup(id, false, requestId),
  );
  const remove = useCommand((requestId: string, id: string) => ipc.deleteBackup(id, requestId));
  const working = restore.pending || remove.pending;
  const error = restore.error ?? remove.error;

  return (
    <Panel title={t.deletedTitle}>
      <p className={styles.hint}>{t.deletedHint}</p>
      {working && <p role="status">{t.working}</p>}
      {message !== null && !working && <Notice tone="info">{message}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <ul className={styles.list}>
        {backups.map((backup) => {
          const canReturn = backup.scope.kind === 'installation' && backup.versionId !== undefined;
          return (
            <li key={backup.id} className={styles.row}>
              <div>
                <strong>{backup.instanceName}</strong>
                <span className={styles.tag}>{t.reason[backup.reason]}</span>
                <span className={styles.hint}>
                  {scopeLabel(backup.scope)} · {when(backup.createdAt)} · {backup.versionLabel}
                </span>
                <span className={styles.hint}>
                  {t.size({
                    files: backup.fileCount,
                    total: formatBytes(backup.totalBytes),
                    added: formatBytes(backup.newBytes),
                  })}
                </span>
                {confirming === backup.id && (
                  <Notice tone="warning">
                    <p>{t.confirmDelete({ date: when(backup.createdAt) })}</p>
                    <div className={forms.actions}>
                      <Button
                        variant="primary"
                        disabled={busy || working}
                        onClick={() => {
                          setConfirming(null);
                          void remove.run(backup.id).then((freed) => {
                            if (freed === undefined) return;
                            setMessage(t.deleted({ size: formatBytes(freed) }));
                            after();
                          });
                        }}
                      >
                        {t.delete}
                      </Button>
                      <Button
                        onClick={() => {
                          setConfirming(null);
                        }}
                      >
                        {strings.createForm.cancel}
                      </Button>
                    </div>
                  </Notice>
                )}
              </div>
              <div className={styles.actions}>
                {canReturn && (
                  <Button
                    variant="primary"
                    disabled={busy || working}
                    onClick={() => {
                      void restore.run(backup.id).then((done) => {
                        if (done === undefined) return;
                        setMessage(t.broughtBack({ name: backup.instanceName }));
                        after();
                      });
                    }}
                  >
                    {t.bringBack}
                  </Button>
                )}
                <Button
                  disabled={busy || working}
                  onClick={() => {
                    setConfirming(backup.id);
                  }}
                >
                  {t.delete}
                </Button>
              </div>
            </li>
          );
        })}
      </ul>
    </Panel>
  );
}
