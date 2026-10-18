import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { UpdateView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { notesFor } from './notes';
import styles from './Updates.module.css';

const t = strings.updates;

/** Once after an update: what the new version brings (D-063). */
export function WhatsNew({ update }: { update: UpdateView }) {
  const dismiss = useCommand(() => ipc.dismissWhatsNew());
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
