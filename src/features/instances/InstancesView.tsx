import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { AppSnapshot } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { CrashHelp } from './CrashHelp';
import { CreateInstanceForm } from './CreateInstanceForm';
import { InstallPanel } from './InstallPanel';
import { InstanceDetails } from './InstanceDetails';
import { InstanceList } from './InstanceList';
import styles from './Instances.module.css';

interface InstancesViewProps {
  snapshot: AppSnapshot;
  /** Opens the settings of an installation on the Settings screen. */
  onEditSettings: (instanceId: string) => void;
  onOpenBackups?: () => void;
  /** Opens the new-installation form with this game version chosen (from the feed). */
  createVersion?: string | null;
}

const t = strings.instances;

export function InstancesView({
  snapshot,
  onEditSettings,
  onOpenBackups = () => undefined,
  createVersion = null,
}: InstancesViewProps) {
  const [creating, setCreating] = useState(createVersion !== null);
  const [deleted, setDeleted] = useState<string | null>(null);
  const select = useCommand((requestId: string, id: string) =>
    ipc.setAppSettings({ selectedInstanceId: id }, requestId),
  );
  const selectedId = snapshot.settings.selectedInstanceId;
  const selected = snapshot.instances.find((i) => i.id === selectedId) ?? null;
  // The explanation stays until the installation's game starts again.
  const lastRun = snapshot.lastRun;
  const crashed =
    selected !== null &&
    snapshot.currentRun === null &&
    lastRun !== null &&
    lastRun.instanceId === selected.id &&
    lastRun.diagnosis !== null
      ? { ...lastRun, diagnosis: lastRun.diagnosis }
      : null;

  return (
    <div className={styles.layout}>
      <section className={styles.column} aria-labelledby="installations-heading">
        <h2 id="installations-heading">{t.title}</h2>
        <InstanceList
          instances={snapshot.instances}
          selectedId={selectedId}
          disabled={select.pending || !snapshot.settings.writable}
          onSelect={(id) => {
            setDeleted(null);
            void select.run(id);
          }}
        />
        {select.error !== null && <Notice tone="danger">{select.error}</Notice>}
        {!creating && (
          <div>
            <Button
              variant="primary"
              onClick={() => {
                setCreating(true);
              }}
            >
              {t.create}
            </Button>
          </div>
        )}
      </section>
      <div className={styles.column}>
        {deleted !== null && <Notice tone="info">{deleted}</Notice>}
        {creating && (
          <CreateInstanceForm
            network={snapshot.network}
            fixedHeapMib={snapshot.settings.defaultMaxHeapMib}
            initialVersion={createVersion}
            onDone={(createdId) => {
              setCreating(false);
              // The new installation is the one to install and play next.
              if (createdId !== undefined && createdId !== selectedId) {
                void select.run(createdId);
              }
            }}
          />
        )}
        {selected !== null && (
          <>
            <InstallPanel
              key={`install-${selected.id}`}
              instance={selected}
              account={snapshot.account}
              activeOperation={snapshot.activeOperation}
              progress={snapshot.operationProgress}
              recentOperations={snapshot.recentOperations}
              queue={snapshot.queue}
              network={snapshot.network}
            />
            {crashed !== null && (
              <CrashHelp
                key={`crash-${crashed.runId}`}
                instance={selected}
                run={crashed}
                onEditSettings={() => {
                  onEditSettings(selected.id);
                }}
                onOpenBackups={onOpenBackups}
                busy={snapshot.activeOperation !== null}
              />
            )}
            <InstanceDetails
              key={selected.id}
              instance={selected}
              onEditSettings={() => {
                onEditSettings(selected.id);
              }}
              busy={snapshot.activeOperation !== null}
              onDeleted={setDeleted}
            />
          </>
        )}
      </div>
    </div>
  );
}
