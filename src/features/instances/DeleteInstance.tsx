import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, UserData } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';
import { contents, hasUserData } from './userData';

const t = strings.deleteInstance;

/**
 * Deletes an installation (D-055). It first looks at what the game folder
 * holds; with worlds, mods, packs or screenshots it offers a backup of the
 * whole installation, which can bring it back later from Backups.
 */
export function DeleteInstance({
  instance,
  busy,
  onDeleted,
  askNow = false,
  onClose,
}: {
  instance: InstanceSummary;
  /** Something runs; deleting waits until it ends. */
  busy: boolean;
  onDeleted: (message: string) => void;
  /** Looks at the folder at once and asks, as when "Delete installation" is chosen in a menu. */
  askNow?: boolean;
  /** Closes the question for good: Cancel calls it instead of going back to the button. */
  onClose?: (() => void) | undefined;
}) {
  const [data, setData] = useState<UserData | null>(null);
  const [checking, setChecking] = useState(askNow);
  const [checkError, setCheckError] = useState<string | null>(null);
  const remove = useCommand((requestId: string, backup: boolean) =>
    ipc.deleteInstance(instance.id, backup, requestId),
  );
  const disabled = busy || instance.busy || remove.pending;

  // Chosen from a menu: the question comes straight away, without a second button.
  useEffect(() => {
    if (!askNow) return;
    let live = true;
    ipc
      .inspectInstance(instance.id)
      .then((found) => {
        if (live) setData(found);
      })
      .catch((error: unknown) => {
        if (live) setCheckError(describeIpcError(error));
      })
      .finally(() => {
        if (live) setChecking(false);
      });
    return () => {
      live = false;
    };
  }, [askNow, instance.id]);

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
          {onClose !== undefined && <Button onClick={onClose}>{t.cancel}</Button>}
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
      {busy && !remove.pending && <p>{t.busy}</p>}
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
            onClose?.();
          }}
        >
          {t.cancel}
        </Button>
      </div>
      {remove.pending && <p role="status">{t.working}</p>}
    </Notice>
  );
}
