import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { AppSnapshot } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { CreateInstanceForm } from './CreateInstanceForm';
import { InstallPanel } from './InstallPanel';
import { InstanceDetails } from './InstanceDetails';
import { InstanceList } from './InstanceList';
import styles from './Instances.module.css';

interface InstancesViewProps {
  snapshot: AppSnapshot;
}

const t = strings.instances;

export function InstancesView({ snapshot }: InstancesViewProps) {
  const [creating, setCreating] = useState(false);
  const select = useCommand((requestId: string, id: string) =>
    ipc.setAppSettings({ selectedInstanceId: id }, requestId),
  );
  const selectedId = snapshot.settings.selectedInstanceId;
  const selected = snapshot.instances.find((i) => i.id === selectedId) ?? null;

  return (
    <div className={styles.layout}>
      <section className={styles.column} aria-labelledby="installations-heading">
        <h2 id="installations-heading">{t.title}</h2>
        <InstanceList
          instances={snapshot.instances}
          selectedId={selectedId}
          disabled={select.pending || !snapshot.settings.writable}
          onSelect={(id) => {
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
        {creating && (
          <CreateInstanceForm
            versions={snapshot.supportedVersions}
            onDone={() => {
              setCreating(false);
            }}
          />
        )}
        {selected !== null && (
          <>
            <InstallPanel
              key={`install-${selected.id}`}
              instance={selected}
              activeOperation={snapshot.activeOperation}
              progress={snapshot.operationProgress}
              recentOperations={snapshot.recentOperations}
            />
            <InstanceDetails key={selected.id} instance={selected} memory={snapshot.memory} />
          </>
        )}
      </div>
    </div>
  );
}
