import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { LiveStatus } from '../../components/LiveStatus';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { Switch } from '../../components/Switch';
import { Tag } from '../../components/Tag';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, BackupInfo, BackupOverview, BackupScope } from '../../lib/contracts';
import { formatBytes, toInt } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { isolate, numberLocale, strings } from '../../lib/strings';
import { shortWhen } from '../../lib/time';
import { useCommand } from '../../lib/useCommand';
import { InstallationSelect } from '../instances/InstallationSelect';
import styles from './Backups.module.css';

const t = strings.backups;

/** Automatic backups kept per world at most (`backup::MAX_KEEP`). */
const KEEP_MAX = 100;

/** The full date and time, for the questions. */
function when(at: string): string {
  return new Date(at).toLocaleString(numberLocale());
}

/** "Today, 19:55", "18 October, 21:10": the time on older days too, as a day can have
 * several backups. */
const shortDate = (at: string, capital = true) =>
  isolate(shortWhen(at, { time: 'always', capital }));

function scopeLabel(scope: BackupScope): string {
  return scope.kind === 'world' ? t.scopeWorld({ name: isolate(scope.name) }) : t.scopeInstallation;
}

/** "42 files · 5.0 MiB · 1.0 MiB new". */
const sizeOf = (backup: BackupInfo) =>
  t.size({
    files: backup.fileCount,
    total: isolate(formatBytes(backup.totalBytes)),
    added: isolate(formatBytes(backup.newBytes)),
  });

/** The panel a press came from: its line says what the press did. */
type Where = 'worlds' | 'list' | 'deleted';

/** A question waiting for the user: restore in place or delete. */
type Asking = { action: 'restore' | 'delete'; backup: BackupInfo; at: Where } | null;

/** Backup settings: automatic backups after playing and how many to keep. */
function BackupSettings({
  snapshot,
  storeBytes,
}: {
  snapshot: AppSnapshot;
  storeBytes: number | null;
}) {
  const settings = snapshot.settings;
  // What the field shows: the saved number until it is edited (no remount after a save,
  // so Save keeps the focus).
  const [keep, setKeep] = useState<string | null>(null);
  // Saves refused for a number out of range: each one says so again.
  const [refused, setRefused] = useState(0);
  const invalid = refused > 0;
  const errorId = useId();
  const update = useCommand(
    (requestId: string, patch: { backupAfterPlay?: boolean; backupKeep?: number }) =>
      ipc.setAppSettings(patch, requestId),
  );
  const value = keep ?? String(settings.backupKeep);
  const disabled = !settings.writable;
  return (
    <Panel>
      <p className={styles.hint}>{t.hint}</p>
      {storeBytes !== null && (
        <p className={styles.hint}>{t.storage({ size: isolate(formatBytes(storeBytes)) })}</p>
      )}
      <Switch
        label={t.afterPlay}
        hint={t.afterPlayHint}
        checked={settings.backupAfterPlay}
        disabled={disabled}
        pending={update.pending}
        onChange={(on) => void update.run({ backupAfterPlay: on })}
      />
      <form
        className={styles.keep}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          if (update.pending) return;
          // Junk never reaches the settings: only a whole number in the range goes.
          const kept = toInt(value);
          if (!(kept >= 1 && kept <= KEEP_MAX)) {
            setRefused((n) => n + 1);
            return;
          }
          setRefused(0);
          if (kept === settings.backupKeep) {
            setKeep(null);
            return;
          }
          void update.run({ backupKeep: kept }).then((done) => {
            if (done !== undefined) setKeep(null);
          });
        }}
      >
        <Field label={t.keep} hint={t.keepHint}>
          {(props) => (
            <div className={styles.keepRow}>
              <input
                {...props}
                aria-describedby={
                  [props['aria-describedby'], invalid ? errorId : undefined]
                    .filter(Boolean)
                    .join(' ') || undefined
                }
                className={[forms.input, styles.number].join(' ')}
                type="number"
                inputMode="numeric"
                min={1}
                max={KEEP_MAX}
                dir="ltr"
                value={value}
                disabled={disabled}
                readOnly={update.pending}
                aria-invalid={invalid || undefined}
                onChange={(event) => {
                  setKeep(event.target.value);
                  setRefused(0);
                }}
              />
              <Button
                type="submit"
                size="small"
                disabled={disabled}
                aria-disabled={update.pending || undefined}
              >
                {t.save}
              </Button>
            </div>
          )}
        </Field>
        {invalid && (
          <div key={refused} id={errorId}>
            <Notice tone="danger">{t.keepRange({ max: KEEP_MAX })}</Notice>
          </div>
        )}
      </form>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
    </Panel>
  );
}

/** How a press may go: its own work keeps the buttons focusable, a wait set by
 * something else (a game, an install) disables them. */
interface Pressable {
  own: boolean;
  waiting: boolean;
}

/** A world of the installation, with its last backup and Back up. */
function WorldRow({
  name,
  last,
  own,
  waiting,
  onBackUp,
}: Pressable & { name: string; last: BackupInfo | undefined; onBackUp: () => void }) {
  const nameId = useId();
  return (
    <li className={styles.item}>
      <span className={styles.itemName}>
        <bdi id={nameId}>{name}</bdi>
      </span>
      <span className={styles.itemMeta}>
        {last === undefined ? t.never : t.lastBackup({ date: shortDate(last.createdAt, false) })}
      </span>
      <span className={styles.itemActions}>
        <Button
          size="small"
          aria-describedby={nameId}
          disabled={waiting}
          aria-disabled={own || undefined}
          onClick={onBackUp}
        >
          <Icon name="copies" size={18} />
          {t.backUp}
        </Button>
      </span>
    </li>
  );
}

/** A backup: what it holds and why it was made, its actions, then their question. */
function BackupRow({
  backup,
  orphan = false,
  own,
  waiting,
  onAsk,
  onRestoreCopy,
  onBringBack,
  question,
}: Pressable & {
  backup: BackupInfo;
  /** Of a deleted installation: named by it, brought back or deleted (D-055). */
  orphan?: boolean;
  onAsk: (action: 'restore' | 'delete', button: HTMLButtonElement) => void;
  onRestoreCopy?: () => void;
  onBringBack?: () => void;
  question: ReactNode;
}) {
  const titleId = useId();
  const whenId = useId();
  // Backups of one world differ by their date: the buttons say both.
  const press = {
    size: 'small',
    'aria-describedby': `${titleId} ${whenId}`,
    disabled: waiting,
    'aria-disabled': own || undefined,
  } as const;
  // Only a whole installation with its version can come back; its worlds only go.
  const canReturn =
    orphan && backup.scope.kind === 'installation' && backup.versionId !== undefined;
  return (
    <li className={[styles.item, styles.stack].join(' ')}>
      <span className={styles.itemName}>
        {orphan ? (
          <bdi id={titleId}>{backup.instanceName}</bdi>
        ) : (
          <span id={titleId}>{scopeLabel(backup.scope)}</span>
        )}
        <Tag>{t.reason[backup.reason]}</Tag>
      </span>
      <span className={styles.itemMeta}>
        <span id={whenId}>
          {orphan && `${scopeLabel(backup.scope)} · `}
          {shortDate(backup.createdAt)}
        </span>{' '}
        · <bdi>{backup.versionLabel}</bdi> · <span>{sizeOf(backup)}</span>
      </span>
      <span className={styles.itemActions}>
        {canReturn && (
          <Button {...press} onClick={onBringBack}>
            {t.bringBack}
          </Button>
        )}
        {!orphan && (
          <Button
            {...press}
            onClick={(event) => {
              onAsk('restore', event.currentTarget);
            }}
          >
            {t.restore}
          </Button>
        )}
        {/* A whole installation is restored only in its place. */}
        {!orphan && backup.scope.kind === 'world' && (
          <Button {...press} onClick={onRestoreCopy}>
            {t.restoreCopy}
          </Button>
        )}
        <Button
          {...press}
          variant="ghost"
          onClick={(event) => {
            onAsk('delete', event.currentTarget);
          }}
        >
          {t.delete}
        </Button>
      </span>
      {question !== null && <div className={styles.itemWide}>{question}</div>}
    </li>
  );
}

/**
 * Incremental backups of worlds and whole installations (D-047): back up now,
 * restore in place or as a copy, delete, see the space used, and bring back a
 * deleted installation (D-055).
 */
export function BackupsView({ snapshot }: { snapshot: AppSnapshot }) {
  const { instances } = snapshot;
  const selectId = useId();
  const listHeadingId = useId();
  const deletedHeadingId = useId();
  const wholeHintId = useId();
  const [chosen, setChosen] = useState(snapshot.settings.selectedInstanceId);
  const [overview, setOverview] = useState<BackupOverview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [asking, setAsking] = useState<Asking>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [where, setWhere] = useState<Where | null>(null);
  // A press here started the operation the snapshot shows (see `own`).
  const [startedHere, setStartedHere] = useState(false);
  // A press with the focus on the screen: while it goes, a row that leaves with the focus
  // hands it on (a deleted backup, an installation brought back). After a mouse press in
  // macOS WebKit the focus stays on the page, and then nothing moves.
  const [handOn, setHandOn] = useState(false);
  // The panel of the last question, for the focus when its button cannot take it back.
  const [askedAt, setAskedAt] = useState<Where>('list');
  const heading = useRef<HTMLHeadingElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const deletedRef = useRef<HTMLUListElement>(null);
  // Restore or Delete that asked: the focus goes back to it when the question closes.
  const opener = useRef<HTMLButtonElement | null>(null);
  // The press has its answer: the next list shown is the one after it.
  const settled = useRef(false);
  // Only the newest answer counts: an older one may arrive after it.
  const asked = useRef(0);
  const instance = instances.find((i) => i.id === chosen) ?? instances[0];

  const reload = useCallback(() => {
    const ask = ++asked.current;
    return ipc
      .listBackups()
      .then((found) => {
        if (ask !== asked.current) return;
        setOverview(found);
        setLoadError(null);
        if (settled.current) {
          settled.current = false;
          setHandOn(false);
        }
      })
      .catch((error: unknown) => {
        if (ask === asked.current) setLoadError(describeIpcError(error));
      });
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);
  // Again when an operation ends: automatic backups run after a game session.
  const activeId = snapshot.activeOperation?.operationId ?? null;
  const wasActive = useRef(activeId);
  useEffect(() => {
    const was = wasActive.current;
    wasActive.current = activeId;
    if (was !== null && activeId === null) void reload();
  }, [activeId, reload]);

  const create = useCommand((requestId: string, id: string, scope: BackupScope) =>
    ipc.createBackup(id, scope, requestId),
  );
  const restore = useCommand((requestId: string, id: string, asCopy: boolean) =>
    ipc.restoreBackup(id, asCopy, requestId),
  );
  const remove = useCommand((requestId: string, id: string) => ipc.deleteBackup(id, requestId));
  const working = create.pending || restore.pending || remove.pending;
  // Each command takes the launcher's activity, so the snapshot shows this screen's own
  // backup too, and may still show it after the answer: until it leaves, the pressed
  // button stays focusable (a disabled one loses the focus in WebView2 and WebKit).
  const operation = snapshot.activeOperation;
  const ours = operation !== null && (operation.kind === 'backup' || operation.kind === 'restore');
  if (startedHere && !working && !ours) setStartedHere(false);
  const own = working || (startedHere && ours);
  const waiting = operation !== null && !own;
  const error = create.error ?? restore.error ?? remove.error;

  useFocusBack(
    asking === null ? null : `${asking.action}:${asking.backup.id}`,
    opener,
    askedAt === 'deleted' ? deletedRef : listRef,
    heading,
  );

  /** A press from a panel: what the others said goes, and its line says it is working. */
  const start = (at: Where) => {
    setStartedHere(true);
    setWhere(at);
    setMessage(null);
    create.reset();
    restore.reset();
    remove.reset();
    settled.current = false;
    setHandOn(root.current?.contains(document.activeElement) ?? false);
  };
  /** What a press did; the list is asked again, and the rows it changes hand the focus on. */
  const after = (text: string | null) => {
    if (text === null) {
      setHandOn(false);
      return;
    }
    setMessage(text);
    settled.current = true;
    void reload();
  };
  const backUp = (id: string, scope: BackupScope) => {
    if (own) return;
    start('worlds');
    void create.run(id, scope).then((done) => {
      after(done ? t.created({ what: scopeLabel(done.scope) }) : null);
    });
  };
  const askAbout = (
    backup: BackupInfo,
    at: Where,
    action: 'restore' | 'delete',
    button: HTMLButtonElement,
  ) => {
    if (own) return;
    opener.current = button;
    setAskedAt(at);
    setAsking({ action, backup, at });
  };
  const restoreCopy = (backup: BackupInfo) => {
    if (own) return;
    start('list');
    void restore.run(backup.id, true).then((done) => {
      after(done?.world ? t.restoredCopy({ name: isolate(done.world) }) : null);
    });
  };
  const bringBack = (backup: BackupInfo) => {
    if (own) return;
    start('deleted');
    void restore.run(backup.id, false).then((done) => {
      after(done === undefined ? null : t.broughtBack({ name: isolate(backup.instanceName) }));
    });
  };
  const question = (backup: BackupInfo) => {
    if (asking?.backup.id !== backup.id) return null;
    const { action, at } = asking;
    return (
      <Confirm
        label={at === 'deleted' ? backup.instanceName : scopeLabel(backup.scope)}
        text={
          action === 'restore'
            ? t.confirmRestore({
                what: scopeLabel(backup.scope),
                date: isolate(when(backup.createdAt)),
              })
            : t.confirmDelete({ date: isolate(when(backup.createdAt)) })
        }
        action={action === 'restore' ? t.restore : t.delete}
        actionVariant={action === 'restore' ? 'primary' : 'danger'}
        cancel={strings.createForm.cancel}
        pending={false}
        held={own || waiting}
        onYes={() => {
          setAsking(null);
          start(at);
          if (action === 'restore') {
            void restore.run(backup.id, false).then((done) => {
              after(done ? t.restored : null);
            });
          } else {
            void remove.run(backup.id).then((freed) => {
              after(freed === undefined ? null : t.deleted({ size: isolate(formatBytes(freed)) }));
            });
          }
        }}
        onNo={() => {
          setAsking(null);
        }}
      />
    );
  };
  // What the last press did, in its own panel: seen where the press was.
  const said = (at: Where) => (
    <>
      <LiveStatus>{where !== at ? null : working ? t.working : message}</LiveStatus>
      {where === at && error !== null && <Notice tone="danger">{error}</Notice>}
    </>
  );
  const loading = overview === null && loadError === null && (
    <p className={styles.hint}>{t.loading}</p>
  );

  const all = overview?.backups ?? [];
  const backups = instance === undefined ? [] : all.filter((b) => b.instanceId === instance.id);
  // Backups of deleted installations (D-055).
  const orphaned = all.filter((b) => !instances.some((i) => i.id === b.instanceId));
  const worlds =
    instance === undefined
      ? []
      : (overview?.worlds.find((w) => w.instanceId === instance.id)?.worlds ?? []);
  const lastOf = (name: string) =>
    backups.find((b) => b.scope.kind === 'world' && b.scope.name === name);
  useFocusBack(
    handOn ? [...backups, ...orphaned].map((b) => b.id).join(' ') : null,
    where === 'deleted' ? deletedRef : listRef,
    heading,
  );

  return (
    <Screen
      title={strings.nav.backups}
      headingRef={heading}
      actions={
        // Above both columns, as it chooses for both, and as wide as its longest name.
        instance !== undefined && (
          <InstallationSelect
            id={selectId}
            label={t.installation}
            instances={instances}
            value={instance.id}
            onChange={(id) => {
              if (id === null) return;
              setChosen(id);
              setAsking(null);
              if (own) return;
              // What was said about another installation goes with it.
              setWhere(null);
              setMessage(null);
              create.reset();
              restore.reset();
              remove.reset();
            }}
            fit
          />
        )
      }
    >
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {waiting && <Notice tone="info">{t.busy}</Notice>}
      <div ref={root} className={styles.columns}>
        <div className={styles.column}>
          <BackupSettings snapshot={snapshot} storeBytes={overview?.storeBytes ?? null} />
          {instance === undefined ? (
            <Panel title={t.worlds} level={3}>
              <p className={styles.hint}>{strings.settings.noInstallations}</p>
            </Panel>
          ) : (
            <Panel title={t.worlds} level={3}>
              {overview !== null &&
                (worlds.length === 0 ? (
                  <p className={styles.hint}>{t.noWorlds}</p>
                ) : (
                  <ul className={styles.items}>
                    {worlds.map((name) => (
                      <WorldRow
                        key={name}
                        name={name}
                        last={lastOf(name)}
                        own={own}
                        waiting={waiting}
                        onBackUp={() => {
                          backUp(instance.id, { kind: 'world', name });
                        }}
                      />
                    ))}
                  </ul>
                ))}
              <div className={styles.whole}>
                <Button
                  size="small"
                  aria-describedby={wholeHintId}
                  disabled={waiting}
                  aria-disabled={own || undefined}
                  onClick={() => {
                    backUp(instance.id, { kind: 'installation' });
                  }}
                >
                  <Icon name="copies" size={18} />
                  {t.whole}
                </Button>
                <span id={wholeHintId} className={styles.hint}>
                  {t.wholeHint}
                </span>
              </div>
              {said('worlds')}
            </Panel>
          )}
        </div>
        <div className={styles.column}>
          {instance !== undefined && (
            <Panel
              title={t.list}
              level={3}
              headingId={listHeadingId}
              count={overview === null ? undefined : backups.length}
            >
              {said('list')}
              {loading}
              {overview !== null && backups.length === 0 && <p className={styles.hint}>{t.none}</p>}
              {backups.length > 0 && (
                <ul
                  ref={listRef}
                  className={[styles.items, styles.stackList].join(' ')}
                  aria-labelledby={listHeadingId}
                  tabIndex={-1}
                >
                  {backups.map((backup) => (
                    <BackupRow
                      key={backup.id}
                      backup={backup}
                      own={own}
                      waiting={waiting}
                      onAsk={(action, button) => {
                        askAbout(backup, 'list', action, button);
                      }}
                      onRestoreCopy={() => {
                        restoreCopy(backup);
                      }}
                      question={question(backup)}
                    />
                  ))}
                </ul>
              )}
            </Panel>
          )}
          {/* After the last one is brought back, the panel stays while it says so. */}
          {(orphaned.length > 0 ||
            (where === 'deleted' && (own || message !== null || error !== null))) && (
            <Panel title={t.deletedTitle} level={3} headingId={deletedHeadingId}>
              <p className={styles.hint}>{t.deletedHint}</p>
              {said('deleted')}
              {orphaned.length > 0 && (
                <ul
                  ref={deletedRef}
                  className={[styles.items, styles.stackList].join(' ')}
                  aria-labelledby={deletedHeadingId}
                  tabIndex={-1}
                >
                  {orphaned.map((backup) => (
                    <BackupRow
                      key={backup.id}
                      backup={backup}
                      orphan
                      own={own}
                      waiting={waiting}
                      onAsk={(action, button) => {
                        askAbout(backup, 'deleted', action, button);
                      }}
                      onBringBack={() => {
                        bringBack(backup);
                      }}
                      question={question(backup)}
                    />
                  ))}
                </ul>
              )}
            </Panel>
          )}
        </div>
      </div>
    </Screen>
  );
}
