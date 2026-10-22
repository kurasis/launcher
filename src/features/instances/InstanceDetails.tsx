import { useId, useRef, useState, type ReactNode } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  ActiveActivity,
  GameFolder,
  InstanceSummary,
  QueuedOperation,
  RunSummary,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { javaLabel } from '../settings/javaLabel';
import { InstanceHeader } from './InstanceHeader';
import styles from './Instances.module.css';
import { lastRunLine } from './lastRunLine';
import { gameLabel } from './loaderLabel';

interface InstanceDetailsProps {
  instance: InstanceSummary;
  /** Opens this installation's settings on the Settings screen. */
  onEditSettings: () => void;
  /** Opens what the installation holds in the Mods tab (D-066). */
  onOpenContent?: (() => void) | undefined;
  onOpenBackups?: (() => void) | undefined;
  /** Something runs, so the installation cannot be deleted now. */
  busy?: boolean;
  onDeleted?: (message: string) => void;
  /** What runs now; Verify files waits while this installation's own operation runs. */
  activeOperation?: ActiveActivity | null;
  /** Installs and repairs waiting their turn (D-049). */
  queue?: QueuedOperation[];
  /** The session's last finished game, for how long the last game ran. */
  lastRun?: RunSummary | null;
  /** The state, progress and crash help, between the toolbar and the facts. */
  children?: ReactNode;
}

const t = strings.details;

/** The folders offered as links, besides the game folder itself (in the toolbar). */
type ShownFolder = Extract<GameFolder, 'mods' | 'saves' | 'resourcepacks' | 'shaderpacks' | 'logs'>;

/**
 * The chosen installation on one surface: its head, the actions, its state, then its
 * facts, folders and notes. Its settings are edited on the Settings screen.
 */
export function InstanceDetails({
  instance,
  onEditSettings,
  onOpenContent,
  onOpenBackups,
  busy = false,
  onDeleted = () => undefined,
  activeOperation = null,
  queue = [],
  lastRun = null,
  children,
}: InstanceDetailsProps) {
  const headingId = useId();
  const detailsId = useId();
  const foldersId = useId();
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const gameFolder = useCommand((requestId: string, id: string) =>
    ipc.openInstanceFolder(id, requestId, null),
  );
  const openFolder = useCommand((requestId: string, id: string, folder: GameFolder) =>
    ipc.openInstanceFolder(id, requestId, folder),
  );
  const mine = activeOperation?.instanceId === instance.id ? activeOperation : null;
  const queued = queue.find((q) => q.instanceId === instance.id) ?? null;
  const last = instance.lastResult;
  // The head tells about the last game when it can; otherwise its date stays here.
  const playedInHead = lastRunLine(instance, lastRun) !== null;
  // Mods and shader packs only matter with a mod loader.
  const folders: ShownFolder[] =
    instance.loader === null
      ? ['saves', 'resourcepacks', 'logs']
      : ['mods', 'saves', 'resourcepacks', 'shaderpacks', 'logs'];
  const toolbarError = repair.error ?? gameFolder.error;

  return (
    <section className={styles.panel} aria-labelledby={headingId}>
      <InstanceHeader
        instance={instance}
        headingId={headingId}
        lastRun={lastRun}
        busy={busy}
        onDeleted={onDeleted}
      />
      <div className={styles.toolbar}>
        {mine === null && instance.health === 'ready' && (
          <Button
            disabled={repair.pending || queued !== null}
            onClick={() => void repair.run(instance.id)}
          >
            <Icon name="check" size={18} />
            {strings.install.verify}
          </Button>
        )}
        {onOpenContent !== undefined && (
          <Button onClick={onOpenContent}>
            <Icon name="mods" size={18} />
            {t.content}
          </Button>
        )}
        <Button onClick={onEditSettings}>
          <Icon name="settings" size={18} />
          {t.title}
        </Button>
        <Button disabled={gameFolder.pending} onClick={() => void gameFolder.run(instance.id)}>
          <Icon name="folder" size={18} />
          {t.folder.game}
        </Button>
        {onOpenBackups !== undefined && (
          <Button onClick={onOpenBackups}>
            <Icon name="db" size={18} />
            {strings.nav.backups}
          </Button>
        )}
      </div>
      {toolbarError !== null && <Notice tone="danger">{toolbarError}</Notice>}
      {children}
      <div className={styles.body}>
        <div className={styles.block}>
          <h4 id={detailsId} className={styles.blockTitle}>
            {t.summaryTitle}
          </h4>
          <dl className={styles.details}>
            <dt>{t.version}</dt>
            <dd>{gameLabel(instance)}</dd>
            <dt>{t.java}</dt>
            <dd>{javaLabel(instance)}</dd>
            <dt>{t.memoryShort}</dt>
            <dd>{t.memoryValue({ mib: instance.maxHeapMib })}</dd>
            <dt>{t.windowShort}</dt>
            <dd>
              {(instance.fullscreen ? t.windowFullscreen : t.windowSize)({
                width: instance.resolution.width,
                height: instance.resolution.height,
              })}
            </dd>
            <dt>{t.launcherSkins}</dt>
            <dd>{instance.launcherSkins ? t.launcherSkinsOn : t.launcherSkinsOff}</dd>
            {instance.jvmArguments !== '' && (
              <>
                <dt>{t.jvmArguments}</dt>
                <dd dir="ltr" className={styles.code}>
                  {instance.jvmArguments}
                </dd>
              </>
            )}
            {instance.gameArguments !== '' && (
              <>
                <dt>{t.gameArguments}</dt>
                <dd dir="ltr" className={styles.code}>
                  {instance.gameArguments}
                </dd>
              </>
            )}
            {!playedInHead && instance.lastPlayedAt !== null && (
              <>
                <dt>{t.lastPlayed}</dt>
                <dd>{new Date(instance.lastPlayedAt).toLocaleString(numberLocale())}</dd>
              </>
            )}
            {last !== null && last.kind !== 'launch' && (
              <>
                <dt>{t.lastResult}</dt>
                <dd>
                  {`${strings.diagnostics.kind[last.kind]}: ${strings.diagnostics.outcome[last.outcome]}${
                    last.exitCode !== undefined
                      ? ` (${t.exitCode({ code: String(last.exitCode) })})`
                      : ''
                  } · ${new Date(last.finishedAt).toLocaleString(numberLocale())}`}
                </dd>
              </>
            )}
          </dl>
        </div>
        <div className={styles.side}>
          <div className={styles.block}>
            <h4 id={foldersId} className={styles.blockTitle}>
              {t.folders}
            </h4>
            <p className={styles.hint}>{t.openFolderHint}</p>
            <div className={styles.folderLinks} role="group" aria-labelledby={foldersId}>
              {folders.map((folder) => (
                <button
                  key={folder}
                  type="button"
                  className={styles.folderLink}
                  disabled={openFolder.pending}
                  onClick={() => void openFolder.run(instance.id, folder)}
                >
                  <Icon name="folder" size={16} />
                  {t.folder[folder]}
                </button>
              ))}
            </div>
            {openFolder.error !== null && <Notice tone="danger">{openFolder.error}</Notice>}
          </div>
          <NotesEditor key={instance.id} instance={instance} />
        </div>
      </div>
    </section>
  );
}

/** The longest notes the core keeps. */
const NOTES_MAX_LENGTH = 4000;

/** The installation's notes: read here, edited on request, also while it is busy. */
function NotesEditor({ instance }: { instance: InstanceSummary }) {
  const headingId = useId();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(instance.notes);
  const [saved, setSaved] = useState(false);
  // Focus goes back to "Edit" when the editor closes.
  const focusEdit = useRef(false);
  const save = useCommand((requestId: string, notes: string) =>
    ipc.setInstanceNotes({ instanceId: instance.id, notes }, requestId),
  );
  const changed = draft.trim() !== instance.notes;

  const stop = () => {
    focusEdit.current = true;
    setEditing(false);
    save.reset();
  };

  if (!editing) {
    return (
      <div className={styles.block}>
        <div className={styles.blockHead}>
          <h4 id={headingId} className={styles.blockTitle}>
            {t.notes}
          </h4>
          <Button
            variant="ghost"
            className={styles.textButton}
            aria-label={t.notesEditLabel}
            ref={(node) => {
              if (node !== null && focusEdit.current) {
                focusEdit.current = false;
                node.focus();
              }
            }}
            onClick={() => {
              setDraft(instance.notes);
              setSaved(false);
              setEditing(true);
            }}
          >
            <Icon name="edit" size={16} />
            {t.notesEdit}
          </Button>
        </div>
        <p
          className={[styles.notesText, instance.notes === '' ? styles.notesEmpty : '']
            .filter(Boolean)
            .join(' ')}
        >
          {instance.notes === '' ? t.notesEmpty : instance.notes}
        </p>
        {saved && (
          <p role="status" className={styles.hint}>
            {t.saved}
          </p>
        )}
      </div>
    );
  }

  return (
    <form
      className={[forms.form, styles.block].join(' ')}
      onSubmit={(event) => {
        event.preventDefault();
        void save.run(draft).then((result) => {
          if (!result) return;
          stop();
          setSaved(true);
        });
      }}
    >
      <Field label={t.notes} hint={t.notesHint}>
        {(props) => (
          <textarea
            {...props}
            className={[forms.input, styles.notes].join(' ')}
            value={draft}
            maxLength={NOTES_MAX_LENGTH}
            rows={5}
            // Opened on purpose with "Edit": typing starts here.
            autoFocus
            onChange={(event) => {
              setDraft(event.target.value.slice(0, NOTES_MAX_LENGTH));
            }}
            onKeyDown={(event) => {
              if (event.key === 'Escape' && !save.pending) {
                event.preventDefault();
                stop();
              }
            }}
          />
        )}
      </Field>
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={save.pending || !changed}>
          {t.notesSave}
        </Button>
        <Button disabled={save.pending} onClick={stop}>
          {t.cancel}
        </Button>
      </div>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
    </form>
  );
}
