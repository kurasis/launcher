import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, ModsExportResult } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from './Dialog';
import styles from './Mods.module.css';

/**
 * Saving an installation as a Modrinth modpack (D-066 stage 3): worlds and
 * game settings only when the player asks, then a save dialog.
 */
export function ExportDialog({
  instance,
  onClose,
}: {
  instance: InstanceSummary;
  onClose: () => void;
}) {
  const t = strings.mods.content;
  const [worlds, setWorlds] = useState(false);
  const [options, setOptions] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ModsExportResult | null>(null);

  const start = () => {
    setWorking(true);
    setError(null);
    setResult(null);
    ipc
      .modsExportPack(instance.id, { worlds, options }, t.exportTitle)
      .then((saved) => {
        if (saved.saved) setResult(saved);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setWorking(false);
      });
  };
  const close = () => {
    if (!working) onClose();
  };

  return (
    <Dialog title={t.exportTitle} onClose={close}>
      <p className={styles.muted}>{t.exportHint}</p>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={worlds}
          disabled={working}
          onChange={(event) => {
            setWorlds(event.target.checked);
          }}
        />
        {t.exportWorlds}
      </label>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={options}
          disabled={working}
          onChange={(event) => {
            setOptions(event.target.checked);
          }}
        />
        {t.exportOptions}
      </label>
      {working && <p role="status">{t.exporting}</p>}
      {result?.report != null && (
        <Notice tone="info">
          {t.exported({
            name: result.fileName ?? '',
            linked: result.report.linked,
            bundled: result.report.bundled,
            size: formatBytes(result.report.bytes),
          })}
        </Notice>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button variant="primary" disabled={working} onClick={start}>
          {t.exportStart}
        </Button>
        {!working && <Button onClick={onClose}>{t.close}</Button>}
      </div>
    </Dialog>
  );
}
