import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Dialog } from '../../components/Dialog';
import dialog from '../../components/Dialog.module.css';
import { Notice } from '../../components/Notice';
import type { InstanceSummary, UserData } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { contents, hasUserData } from './userData';

const t = strings.deleteInstance;

/**
 * Deletes an installation (D-055), as a question in a dialog. It first looks at what
 * the game folder holds; with worlds, mods, packs or screenshots it offers a backup
 * of the whole installation, which can bring it back later from Backups.
 */
export function DeleteInstance({
  instance,
  busy,
  onDeleted,
  onClose,
}: {
  instance: InstanceSummary;
  /** Something runs; deleting waits until it ends. */
  busy: boolean;
  onDeleted: (message: string) => void;
  /** Cancel, Escape: the question goes away and nothing is deleted. */
  onClose: () => void;
}) {
  const [data, setData] = useState<UserData | null>(null);
  const [checkError, setCheckError] = useState<string | null>(null);
  // Each look at the folder; Try again asks once more.
  const [look, setLook] = useState(0);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const bodyId = useId();
  const remove = useCommand((requestId: string, backup: boolean) =>
    ipc.deleteInstance(instance.id, backup, requestId),
  );
  const disabled = busy || instance.busy || remove.pending;

  useEffect(() => {
    let live = true;
    ipc
      .inspectInstance(instance.id)
      .then((found) => {
        if (live) setData(found);
      })
      .catch((error: unknown) => {
        if (live) setCheckError(describeIpcError(error));
      });
    return () => {
      live = false;
    };
  }, [instance.id, look]);

  const run = (backup: boolean) => {
    const name = instance.displayName;
    void remove.run(backup).then((result) => {
      if (result === undefined) return;
      onDeleted(result === null ? t.deleted({ name }) : t.deletedWithBackup({ name }));
    });
  };

  const cancel = () => {
    remove.reset();
    onClose();
  };

  const found = data !== null && hasUserData(data);
  return (
    // Cancel takes the focus: a stray Enter deletes nothing.
    <Dialog
      title={t.confirm({ name: instance.displayName })}
      describedBy={bodyId}
      initialFocus={cancelRef}
      closable={!remove.pending}
      onClose={cancel}
    >
      <div id={bodyId} className={dialog.body}>
        {data === null && checkError === null && <p role="status">{t.checking}</p>}
        {data !== null &&
          (found ? (
            <>
              <p>{t.holds({ size: formatBytes(data.bytes) })}</p>
              <ul className={dialog.list}>
                {contents(data).map((part) => (
                  <li key={part}>{part}</li>
                ))}
              </ul>
              <p>{t.offerBackup}</p>
            </>
          ) : (
            <p>{t.nothing}</p>
          ))}
      </div>
      {checkError !== null && <Notice tone="danger">{checkError}</Notice>}
      {data !== null && <p className={dialog.hint}>{t.shared}</p>}
      {busy && !remove.pending && <p>{t.busy}</p>}
      {remove.error !== null && <Notice tone="danger">{remove.error}</Notice>}
      {remove.pending && <p role="status">{t.working}</p>}
      <div className={dialog.actions}>
        {found && (
          <Button
            variant="primary"
            size="small"
            disabled={disabled}
            onClick={() => {
              run(true);
            }}
          >
            {t.backupAndDelete}
          </Button>
        )}
        {data !== null && (
          <Button
            variant="danger"
            size="small"
            disabled={disabled}
            onClick={() => {
              run(false);
            }}
          >
            {found ? t.deleteWithoutBackup : t.delete}
          </Button>
        )}
        {checkError !== null && (
          <Button
            size="small"
            onClick={() => {
              setCheckError(null);
              setLook((n) => n + 1);
            }}
          >
            {t.retry}
          </Button>
        )}
        <Button
          ref={cancelRef}
          variant="ghost"
          size="small"
          disabled={remove.pending}
          onClick={cancel}
        >
          {t.cancel}
        </Button>
      </div>
    </Dialog>
  );
}
