import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useShownError } from '../../lib/useShownError';
import { Dialog } from '../../components/Dialog';
import styles from './Mods.module.css';

/** Asks before a link from a project's page opens in the browser (D-066). */
export function LinkConfirm({ url, onClose }: { url: string; onClose: () => void }) {
  const t = strings.mods.link;
  const opening = useShownError();
  return (
    <Dialog title={t.title} onClose={onClose}>
      <p>{t.text}</p>
      <p className={styles.url} dir="ltr">
        {url}
      </p>
      {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
      <div className={forms.actions}>
        <Button
          variant="primary"
          onClick={() => {
            opening.clear();
            void ipc.modsOpenLink(url).then(onClose).catch(opening.catcher);
          }}
        >
          {t.open}
        </Button>
        <Button onClick={onClose}>{t.cancel}</Button>
      </div>
    </Dialog>
  );
}
