import { useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { useFocusBack } from '../../components/useFocusBack';
import type { AppSnapshot, LoaderKind } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { ImportDialog } from '../mods/ImportDialog';
import { CrashHelp } from './CrashHelp';
import { CreateInstanceForm } from './CreateInstanceForm';
import { InstallPanel } from './InstallPanel';
import { InstanceDetails } from './InstanceDetails';
import { InstanceList } from './InstanceList';
import { InstanceSettings } from './InstanceSettings';
import styles from './Instances.module.css';

interface InstancesViewProps {
  snapshot: AppSnapshot;
  onOpenBackups?: () => void;
  /** What an installation holds, in the Mods tab (D-066). */
  onOpenContent?: (instanceId: string) => void;
  /**
   * Opens the new-installation form, with this game version (the feed's Install) or
   * mod loader (the command palette) chosen when given.
   */
  createRequest?: CreateRequest | null | undefined;
}

export interface CreateRequest {
  version: string | null;
  loader: LoaderKind | null;
}

const t = strings.instances;

/**
 * Installations (concept A): the list on the inline-start side, the chosen one on a
 * surface beside it, or its settings in place of it. Creating one takes the whole
 * screen until it is done.
 */
export function InstancesView({
  snapshot,
  onOpenBackups = () => undefined,
  onOpenContent,
  createRequest = null,
}: InstancesViewProps) {
  const headingId = useId();
  const [creating, setCreating] = useState(createRequest !== null);
  const [deleted, setDeleted] = useState<string | null>(null);
  const [bringing, setBringing] = useState(false);
  // The chosen installation's settings show in place of its details; another row chosen
  // meanwhile shows its own.
  const [editing, setEditing] = useState(false);
  const settingsHeading = useRef<HTMLHeadingElement>(null);
  const settingsButton = useRef<HTMLButtonElement>(null);
  // Set when the form closes, so focus comes back to the button that opens it.
  const returnFocus = useRef(false);
  const select = useCommand((requestId: string, id: string) =>
    ipc.setAppSettings({ selectedInstanceId: id }, requestId),
  );
  const instances = snapshot.instances;
  const selectedId = snapshot.settings.selectedInstanceId;
  const selected = instances.find((i) => i.id === selectedId) ?? null;
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
  const settingsOpen = editing && selected !== null;
  // The button pressed goes with the view it opened: the settings' title takes the focus,
  // and Back gives it to Installation settings.
  useFocusBack(settingsOpen, settingsHeading, settingsButton);
  const openSettings = () => {
    setEditing(true);
  };

  if (creating) {
    return (
      <CreateInstanceForm
        network={snapshot.network}
        fixedHeapMib={snapshot.settings.defaultMaxHeapMib}
        initialVersion={createRequest?.version ?? null}
        initialLoader={createRequest?.loader ?? null}
        onDone={(createdId) => {
          returnFocus.current = true;
          setCreating(false);
          // A new installation opens on its details, where it is installed.
          setEditing(false);
          // The new installation is the one to install and play next.
          if (createdId !== undefined && createdId !== selectedId) {
            void select.run(createdId);
          }
        }}
      />
    );
  }

  return (
    <section className={styles.screen} aria-labelledby={headingId}>
      <header className={styles.pageHead}>
        <div className={styles.pageTitleRow}>
          <h2 id={headingId} className={styles.pageTitle}>
            {t.title}
          </h2>
          {instances.length > 0 && (
            <span className={styles.count}>
              <span aria-hidden="true">{instances.length.toLocaleString(numberLocale())}</span>
              <span className={styles.visuallyHidden}>{t.count({ count: instances.length })}</span>
            </span>
          )}
        </div>
        <div className={styles.pageActions}>
          <Button
            ref={(node) => {
              if (node !== null && returnFocus.current) {
                returnFocus.current = false;
                node.focus();
              }
            }}
            onClick={() => {
              setCreating(true);
            }}
          >
            <Icon name="plus" size={18} />
            {t.create}
          </Button>
          <Button
            variant="ghost"
            disabled={!snapshot.settings.writable}
            onClick={() => {
              setBringing(true);
            }}
          >
            <Icon name="import" size={18} className={styles.mirror} />
            {strings.mods.bringIn.button}
          </Button>
        </div>
      </header>
      {bringing && (
        <ImportDialog
          onClose={() => {
            setBringing(false);
          }}
          onDone={(id) => {
            // The new installation is the one to play next, shown with its details.
            setEditing(false);
            if (id !== selectedId) void select.run(id);
          }}
        />
      )}
      {deleted !== null && <Notice tone="info">{deleted}</Notice>}
      {instances.length === 0 ? (
        <p className={styles.empty}>{t.empty}</p>
      ) : (
        <div className={styles.layout}>
          <div className={styles.listColumn}>
            <InstanceList
              instances={instances}
              selectedId={selectedId}
              labelledBy={headingId}
              // A choice cannot be saved with read-only settings; finding still works.
              disabled={!snapshot.settings.writable}
              onSelect={(id) => {
                // Rows stay enabled while a choice is saved, so focus stays on the row.
                if (select.pending) return;
                setDeleted(null);
                void select.run(id);
              }}
            />
            {select.error !== null && <Notice tone="danger">{select.error}</Notice>}
          </div>
          <div className={styles.main}>
            {selected === null ? (
              <p className={styles.choose}>{t.choose}</p>
            ) : settingsOpen ? (
              <InstanceSettings
                key={selected.id}
                instance={selected}
                memory={snapshot.memory}
                queued={snapshot.queue.some((q) => q.instanceId === selected.id)}
                headingRef={settingsHeading}
                onBack={() => {
                  setEditing(false);
                }}
              />
            ) : (
              <InstanceDetails
                key={selected.id}
                instance={selected}
                onEditSettings={openSettings}
                settingsRef={settingsButton}
                onOpenContent={
                  onOpenContent === undefined
                    ? undefined
                    : () => {
                        onOpenContent(selected.id);
                      }
                }
                onOpenBackups={onOpenBackups}
                busy={snapshot.activeOperation !== null}
                onDeleted={setDeleted}
                activeOperation={snapshot.activeOperation}
                queue={snapshot.queue}
                lastRun={snapshot.lastRun}
              >
                <InstallPanel
                  key={`install-${selected.id}`}
                  instance={selected}
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
                    onEditSettings={openSettings}
                    onOpenBackups={onOpenBackups}
                    busy={snapshot.activeOperation !== null}
                  />
                )}
              </InstanceDetails>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
