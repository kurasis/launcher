import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { UpdateView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { notesFor } from './notes';
import styles from './Updates.module.css';

const t = strings.updates;

/** Once after an update: what the new version brings, or that the launcher
 * went back because the new version did not start (D-063). */
export function WhatsNew({ update }: { update: UpdateView }) {
  const dismiss = useCommand(() => ipc.dismissWhatsNew());
  if (update.rolledBack !== null) {
    return (
      <Notice tone="warning">
        <p>{t.rolledBack(update.rolledBack)}</p>
        <Button disabled={dismiss.pending} onClick={() => void dismiss.run()}>
          {t.gotIt}
        </Button>
      </Notice>
    );
  }
  if (update.whatsNew === null) return null;
  const notes = notesFor(update.whatsNew.notes);
  return (
    <Notice tone="info">
      <p>
        <strong>{t.whatsNew({ version: update.whatsNew.version })}</strong>
      </p>
      <p className={styles.notes} dir="auto">
        {notes ?? t.noNotes}
      </p>
      <Button disabled={dismiss.pending} onClick={() => void dismiss.run()}>
        {t.gotIt}
      </Button>
    </Notice>
  );
}
