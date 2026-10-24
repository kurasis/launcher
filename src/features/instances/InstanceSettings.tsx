import { useId, type RefObject } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import type { InstanceSummary, MemoryLimits } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { isolate } from '../mods/helpers';
import { InstallationForm } from '../settings/InstallationForm';
import { InstanceJavaForm } from '../settings/InstanceJavaForm';
import { InstanceLoaderForm } from '../settings/InstanceLoaderForm';
import styles from './Instances.module.css';

interface InstanceSettingsProps {
  instance: InstanceSummary;
  memory: MemoryLimits;
  /** An install of this installation waits its turn: a new loader version waits too. */
  queued: boolean;
  headingRef: RefObject<HTMLHeadingElement | null>;
  /** Back to the installation's details. */
  onBack: () => void;
}

const t = strings.details;

/**
 * One installation's settings in place of its details (concept A): memory and window,
 * the mod loader's version and Java, each saved on its own. The same forms are on the
 * Settings screen too. Keyed by instance id by the parent.
 */
export function InstanceSettings({
  instance,
  memory,
  queued,
  headingRef,
  onBack,
}: InstanceSettingsProps) {
  const titleId = useId();
  const folder = useCommand((requestId: string, id: string) =>
    ipc.openInstanceFolder(id, requestId, null),
  );

  return (
    <section className={styles.settings} aria-labelledby={titleId}>
      <div className={styles.settingsHead}>
        <button type="button" className={styles.back} onClick={onBack}>
          <Icon name="back" size={18} className={styles.mirror} />
          {t.backToDetails}
        </button>
        <h3 id={titleId} ref={headingRef} tabIndex={-1} className={styles.settingsTitle}>
          {t.settingsOf({ name: isolate(instance.displayName) })}
        </h3>
        <Button
          size="small"
          className={styles.headEnd}
          aria-disabled={folder.pending || undefined}
          onClick={() => {
            if (!folder.pending) void folder.run(instance.id);
          }}
        >
          <Icon name="folder" size={18} />
          {strings.crash.actions.open_game_folder}
        </Button>
      </div>
      {folder.error !== null && <Notice tone="danger">{folder.error}</Notice>}
      <Panel title={t.memoryWindow} level={4}>
        <InstallationForm instance={instance} memory={memory} />
      </Panel>
      {instance.loader !== null && (
        <InstanceLoaderForm
          instance={instance}
          loader={instance.loader}
          queued={queued}
          level={4}
        />
      )}
      <InstanceJavaForm instance={instance} level={4} />
    </section>
  );
}
