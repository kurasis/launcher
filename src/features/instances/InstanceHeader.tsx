import { useRef, useState } from 'react';
import { MoreMenu } from '../../app/MoreMenu';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { InstanceTile } from '../../components/InstanceTile';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, RunSummary } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { DeleteInstance } from './DeleteInstance';
import { healthTone } from './healthTone';
import styles from './Instances.module.css';
import { lastRunLine } from './lastRunLine';
import { gameLabel } from './loaderLabel';

/** The longest name an installation takes (the core checks it too). */
const NAME_MAX_LENGTH = 64;

interface InstanceHeaderProps {
  instance: InstanceSummary;
  /** The id of the name's heading, which names the whole panel. */
  headingId: string;
  /** The session's last finished game, for how long the last game ran. */
  lastRun: RunSummary | null;
  /** Something runs, so the installation cannot be deleted now. */
  busy: boolean;
  onDeleted: (message: string) => void;
}

/**
 * The chosen installation's head: tile, name, version, state and the last game, with
 * Rename and Delete in a menu: Rename opens under the head, Delete asks in a dialog.
 */
export function InstanceHeader({
  instance,
  headingId,
  lastRun,
  busy,
  onDeleted,
}: InstanceHeaderProps) {
  const [open, setOpen] = useState<'rename' | 'delete' | null>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const line = lastRunLine(instance, lastRun);
  const health = instance.health;

  // The control that had focus goes away with the block: the name takes it.
  const close = () => {
    setOpen(null);
    headingRef.current?.focus();
  };

  return (
    <>
      <header className={styles.head}>
        <InstanceTile id={instance.id} size={56} />
        <div className={styles.headText}>
          <div className={styles.headTitle}>
            <h3 id={headingId} ref={headingRef} tabIndex={-1} className={styles.name}>
              {instance.displayName}
            </h3>
            <bdi className={styles.version}>{gameLabel(instance)}</bdi>
            <StatusBadge
              className={styles.headState}
              tone={healthTone[health]}
              label={instance.busy ? strings.instances.busy : strings.instances.health[health]}
            />
          </div>
          {line !== null && <p className={styles.lastRun}>{line}</p>}
        </div>
        <MoreMenu
          label={strings.instances.actions}
          ariaLabel={strings.instances.actions}
          icon={<Icon name="more" />}
          caret={false}
          current={false}
          buttonClassName={styles.moreButton}
          entries={[
            {
              kind: 'item',
              key: 'rename',
              label: strings.details.rename,
              onSelect: () => {
                setOpen('rename');
              },
            },
            {
              kind: 'item',
              key: 'delete',
              label: strings.deleteInstance.delete,
              onSelect: () => {
                setOpen('delete');
              },
            },
          ]}
        />
      </header>
      {open === 'rename' && (
        <div className={styles.inlineBlock}>
          <RenameForm key={instance.id} instance={instance} onClose={close} />
        </div>
      )}
      {open === 'delete' && (
        <DeleteInstance
          key={instance.id}
          instance={instance}
          busy={busy}
          onDeleted={onDeleted}
          onClose={close}
        />
      )}
    </>
  );
}

/** The installation's name; saved on request, also while it is busy. */
function RenameForm({ instance, onClose }: { instance: InstanceSummary; onClose: () => void }) {
  const [draft, setDraft] = useState(instance.displayName);
  const rename = useCommand((requestId: string, displayName: string) =>
    ipc.renameInstance({ instanceId: instance.id, displayName }, requestId),
  );
  const changed = draft.trim() !== instance.displayName;
  return (
    <form
      className={forms.form}
      onSubmit={(event) => {
        event.preventDefault();
        void rename.run(draft).then((result) => {
          // The new name shows in the head: the form has done its job.
          if (result) onClose();
        });
      }}
    >
      <Field label={strings.details.name}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            value={draft}
            maxLength={NAME_MAX_LENGTH}
            autoComplete="off"
            spellCheck={false}
            // Opened on purpose from the menu: typing starts here.
            autoFocus
            onChange={(event) => {
              setDraft(event.target.value.slice(0, NAME_MAX_LENGTH));
            }}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                onClose();
              }
            }}
          />
        )}
      </Field>
      <div className={forms.actions}>
        <Button
          type="submit"
          variant="primary"
          disabled={rename.pending || !changed || draft.trim() === ''}
        >
          {strings.details.rename}
        </Button>
        <Button disabled={rename.pending} onClick={onClose}>
          {strings.details.cancel}
        </Button>
      </div>
      {rename.error !== null && <Notice tone="danger">{rename.error}</Notice>}
    </form>
  );
}
