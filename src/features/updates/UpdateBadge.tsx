import { useState } from 'react';
import type { UpdateView } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { UpdateDialog } from './UpdateDialog';
import styles from './Updates.module.css';

const t = strings.updates;

/** In the header while an update is there or needs attention (D-063). */
export function UpdateBadge({ update }: { update: UpdateView }) {
  const [open, setOpen] = useState(false);
  if (update.phase === 'idle' || update.version === null) return null;
  const version = update.version;
  const label = {
    available: t.badge.available({ version }),
    downloading: t.badge.downloading({ version }),
    ready: t.badge.ready({ version }),
    notice: t.badge.available({ version }),
    failed: t.badge.failed,
  }[update.phase];
  return (
    <>
      <button
        type="button"
        className={styles.badge}
        onClick={() => {
          setOpen(true);
        }}
      >
        {label}
      </button>
      {open && (
        <UpdateDialog
          update={update}
          onClose={() => {
            setOpen(false);
          }}
        />
      )}
    </>
  );
}
