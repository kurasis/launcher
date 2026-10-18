import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, UserData } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';

const t = strings.deleteInstance;

function hasUserData(data: UserData): boolean {
  return (
    data.worlds.length + data.mods + data.resourcePacks + data.shaderPacks + data.screenshots > 0
  );
}

/** "3 worlds (A, B, C), 12 mods, 5 screenshots" for what the folder holds. */
function contents(data: UserData): string[] {
  const parts: string[] = [];
  if (data.worlds.length > 0) {
    parts.push(t.worlds({ count: data.worlds.length, names: data.worlds.join(', ') }));
  }
  if (data.mods > 0) parts.push(t.mods({ count: data.mods }));
  if (data.resourcePacks > 0) parts.push(t.resourcePacks({ count: data.resourcePacks }));
  if (data.shaderPacks > 0) parts.push(t.shaderPacks({ count: data.shaderPacks }));
  if (data.screenshots > 0) parts.push(t.screenshots({ count: data.screenshots }));
  return parts;
}

/**
 * Deletes an installation (D-055). It first looks at what the game folder
 * holds; with worlds, mods, packs or screenshots it offers a backup of the
 * whole installation, which can bring it back later from Backups.
 */
export function DeleteInstance({
  instance,
  busy,
  onDeleted,
}: {
  instance: InstanceSummary;
  /** Something runs; deleting waits until it ends. */
  busy: boolean;
  onDeleted: (message: string) => void;
}) {
  const [data, setData] = useState<UserData | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const remove = useCommand((requestId: string, backup: boolean) =>
    ipc.deleteInstance(instance.id, backup, requestId),
  );
  const disabled = busy || instance.busy || remove.pending;

  const ask = () => {
    setChecking(true);
    setCheckError(null);
    ipc
      .inspectInstance(instance.id)
      .then(setData)
      .catch((error: unknown) => {
        setCheckError(describeIpcError(error));
      })
      .finally(() => {
        setChecking(false);
      });
  };

  const run = (backup: boolean) => {
    const name = instance.displayName;
    void remove.run(backup).then((result) => {
      if (result === undefined) return;
      onDeleted(result === null ? t.deleted({ name }) : t.deletedWithBackup({ name }));
    });
  };

  if (data === null) {
    return (
      <>
        <div className={forms.actions}>
          <Button disabled={disabled || checking} onClick={ask}>
            {t.delete}
          </Button>
        </div>
        {checkError !== null && <Notice tone="danger">{checkError}</Notice>}
      </>
    );
  }

  const found = hasUserData(data);
  return (
    <Notice tone="warning">
      <p className={styles.strong}>{t.confirm({ name: instance.displayName })}</p>
      {found ? (
        <>
          <p>{t.holds({ size: formatBytes(data.bytes) })}</p>
          <ul className={styles.contents}>
            {contents(data).map((part) => (
              <li key={part}>{part}</li>
            ))}
          </ul>
          <p>{t.offerBackup}</p>
        </>
      ) : (
        <p>{t.nothing}</p>
      )}
      <p className={styles.muted}>{t.shared}</p>
      {busy && <p>{t.busy}</p>}
      {remove.error !== null && <p role="alert">{remove.error}</p>}
      <div className={forms.actions}>
        {found && (
          <Button
            variant="primary"
            disabled={disabled}
            onClick={() => {
              run(true);
            }}
          >
            {t.backupAndDelete}
          </Button>
        )}
        <Button
          disabled={disabled}
          onClick={() => {
            run(false);
          }}
        >
          {found ? t.deleteWithoutBackup : t.delete}
        </Button>
        <Button
          disabled={remove.pending}
          onClick={() => {
            setData(null);
            remove.reset();
          }}
        >
          {t.cancel}
        </Button>
      </div>
      {remove.pending && <p role="status">{t.working}</p>}
    </Notice>
  );
}
