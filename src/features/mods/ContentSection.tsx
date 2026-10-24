import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Choice } from '../../components/Choice';
import { Confirm } from '../../components/Confirm';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Switch } from '../../components/Switch';
import { Tag } from '../../components/Tag';
import { useFocusBack } from '../../components/useFocusBack';
import type {
  ContentIdentified,
  ContentItem,
  ContentUpdate,
  ContentView,
  InstanceSummary,
  ModsFolderCheck,
  ModsNeededBy,
  ModsVersion,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { ProjectIcon } from './CatalogTile';
import { ExportDialog } from './ExportDialog';
import { PublishDialog } from './PublishDialog';
import { isolate, packArchive } from './helpers';
import { PackDialog } from './PackDialog';
import { Problems } from './Problems';
import catalog from './Catalog.module.css';
import styles from './Mods.module.css';

const FOLDERS = ['mods', 'resourcepacks', 'shaderpacks', 'datapacks'] as const;

/** A file's row stays while it is turned off or on (its path gains or loses ".disabled"). */
const rowName = (item: ContentItem) => JSON.stringify([item.world, item.folder, item.name]);

/** A path as when the file is on: an offer or a name found for a file follows it when it is turned off or on. */
const onPath = (path: string) =>
  path.endsWith('.disabled') ? path.slice(0, -'.disabled'.length) : path;

/** Where an update stands while "Update all" runs. */
interface Batch {
  done: number;
  total: number;
  failed: number;
}

interface Props {
  instance: InstanceSummary;
  onOpen: (projectId: string) => void;
  /** Opens the add dialog for a newer version that replaces this file. */
  onUpdate: (update: ContentUpdate, title: string, world: string | null) => void;
  /** Bumped by the parent after an install, to read the folder again. */
  revision: number;
}

function Row({
  item,
  update,
  known,
  ownVersion,
  waiting,
  pending,
  onCheck,
  onToggle,
  onRemove,
  onOpen,
  onUpdate,
}: {
  item: ContentItem;
  update: ContentUpdate | undefined;
  known: ContentIdentified | undefined;
  /** The version a jar the launcher did not install gives itself. */
  ownVersion: string | undefined;
  /** Something else holds the installation (the game runs, it installs): nothing changes. */
  waiting: boolean;
  /** A change on this tab goes: the controls wait for it but keep the focus. */
  pending: boolean;
  /** Which mods need this one, before it is turned off or removed. */
  onCheck: () => Promise<ModsNeededBy | null>;
  onToggle: () => void;
  /** Removes the file, and these files with it. */
  onRemove: (also: string[]) => void;
  onOpen: (projectId: string) => void;
  /** Opens the newer version, named as the row names the file. */
  onUpdate: (title: string) => void;
}) {
  const t = strings.mods.content;
  // Asking before turning a mod off (when others need it) or removing a file.
  const [asking, setAsking] = useState<'off' | 'remove' | null>(null);
  // What asked last: the focus goes back to it when the question closes.
  const [askedBy, setAskedBy] = useState<'off' | 'remove'>('remove');
  const [needs, setNeeds] = useState<ModsNeededBy | null>(null);
  const [checkingNeeds, setCheckingNeeds] = useState(false);
  const [alsoUnused, setAlsoUnused] = useState(true);
  const titleId = useId();
  const switchInput = useRef<HTMLInputElement>(null);
  const removeButton = useRef<HTMLButtonElement>(null);
  useFocusBack(
    asking,
    ...(askedBy === 'off' ? [switchInput, removeButton] : [removeButton, switchInput]),
  );
  const isMod = item.folder === 'mods' && !item.directory;
  const ask = (then: (found: ModsNeededBy | null) => void) => {
    if (!isMod) {
      then(null);
      return;
    }
    setCheckingNeeds(true);
    void onCheck()
      .catch(() => null)
      .then((found) => {
        setCheckingNeeds(false);
        then(found);
      });
  };
  const toggle = () => {
    if (!item.enabled) {
      onToggle();
      return;
    }
    ask((found) => {
      if (found === null || found.neededBy.length === 0) {
        onToggle();
      } else {
        setNeeds(found);
        setAskedBy('off');
        setAsking('off');
      }
    });
  };
  const stopAsking = () => {
    setAsking(null);
    setNeeds(null);
  };
  const neededBy = needs !== null && needs.neededBy.length > 0 ? needs.neededBy : null;
  const unused =
    asking === 'remove' && needs !== null && needs.unused.length > 0 ? needs.unused : null;
  // A file the launcher did not install (a modpack's, the player's) is
  // named as the catalog shows it.
  const title = item.title ?? known?.title ?? item.name;
  // One the catalog does not know says its own version.
  const versionNumber = item.versionNumber ?? known?.versionNumber ?? ownVersion ?? null;
  const project = item.projectId ?? known?.projectId ?? null;
  const source = item.source ?? known?.source ?? null;
  return (
    <li className={styles.item}>
      <ProjectIcon
        hit={{ icon: item.icon ?? known?.icon ?? null, title, slug: item.name }}
        className={styles.itemIcon}
      />
      <span className={styles.itemName}>
        <bdi id={titleId}>{title}</bdi>
        {versionNumber !== null && (
          <span className={styles.itemVersion}>
            <bdi>{versionNumber}</bdi>
          </span>
        )}
        {!item.enabled && <Tag>{t.off}</Tag>}
        {item.dependency && <Tag>{t.dependency}</Tag>}
        {item.fromPack ? <Tag>{t.fromPack}</Tag> : item.projectId === null && <Tag>{t.byHand}</Tag>}
        {update !== undefined && (
          <Tag tone="warning">{t.updateTo({ version: isolate(update.toNumber) })}</Tag>
        )}
      </span>
      <span className={styles.itemMeta}>
        {source !== null && `${isolate(strings.mods.sources[source])} · `}
        <bdi dir="ltr">{item.path}</bdi>
        {/* "1.3 MiB" stays whole in Arabic. */}
        {!item.directory && ` · ${isolate(formatBytes(item.size))}`}
        {item.world !== null && ` · ${t.inWorld({ world: isolate(item.world) })}`}
      </span>
      {/* The buttons repeat in every row: each is described by its row's name. */}
      <span className={styles.itemControls}>
        {!item.directory && (
          <Switch
            className={styles.itemSwitch}
            inputRef={switchInput}
            checked={item.enabled}
            disabled={waiting}
            pending={pending || checkingNeeds || asking !== null}
            onChange={toggle}
            label={
              <>
                <span className={catalog.visuallyHidden}>{t.toggle({ name: isolate(title) })}</span>
                <span aria-hidden="true">{t.on}</span>
              </>
            }
          />
        )}
        <span className={styles.itemActions}>
          {update !== undefined && (
            <Button
              size="small"
              aria-describedby={titleId}
              disabled={waiting}
              aria-disabled={pending || undefined}
              onClick={() => {
                if (!pending) onUpdate(title);
              }}
            >
              {t.update}
            </Button>
          )}
          {project !== null && (
            <Button
              size="small"
              aria-describedby={titleId}
              onClick={() => {
                onOpen(project);
              }}
            >
              {t.open}
            </Button>
          )}
          {!item.directory && asking === null && (
            <Button
              ref={removeButton}
              size="small"
              aria-describedby={titleId}
              disabled={waiting}
              aria-disabled={pending || checkingNeeds || undefined}
              onClick={() => {
                if (pending || checkingNeeds) return;
                ask((found) => {
                  setNeeds(found);
                  setAlsoUnused(true);
                  setAskedBy('remove');
                  setAsking('remove');
                });
              }}
            >
              {t.remove}
            </Button>
          )}
        </span>
      </span>
      {checkingNeeds && (
        <p role="status" className={styles.itemWide}>
          {t.checkingNeeds}
        </p>
      )}
      {asking !== null && (
        <div className={styles.itemWide}>
          <Confirm
            label={title}
            action={asking === 'off' ? t.turnOffAnyway : t.confirmRemove}
            actionVariant={asking === 'off' ? 'primary' : 'danger'}
            cancel={t.cancel}
            // The answer closes the question at once; while something else runs it
            // waits, and Cancel still closes.
            pending={false}
            held={waiting || pending}
            onYes={() => {
              const also = unused !== null && alsoUnused ? unused.map((u) => u.path) : [];
              const wanted = asking;
              stopAsking();
              if (wanted === 'off') onToggle();
              else onRemove(also);
            }}
            onNo={stopAsking}
          >
            {asking === 'remove' && <p>{t.removeQuestion({ name: isolate(title) })}</p>}
            {neededBy !== null && <p>{t.neededBy({ names: neededBy.map(isolate).join(', ') })}</p>}
            {unused !== null && (
              <Choice
                type="checkbox"
                checked={alsoUnused}
                title={t.alsoRemove({ names: unused.map((u) => isolate(u.title)).join(', ') })}
                onChange={(event) => {
                  setAlsoUnused(event.target.checked);
                }}
              />
            )}
          </Confirm>
        </div>
      )}
    </li>
  );
}

/**
 * The content of an installation's game folder (D-066): turn files on or off,
 * remove them, and update what the catalog knows.
 */
export function ContentSection({ instance, onOpen, onUpdate, revision }: Props) {
  const t = strings.mods.content;
  const id = instance.id;
  const [view, setView] = useState<ContentView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  // Offers for the folder as it was at `revision`: a file turned off or on keeps its
  // offer, one removed takes it away.
  const [checked, setChecked] = useState<{
    revision: number;
    updates: ContentUpdate[];
    /** A newer version of the modpack the installation was made from. */
    pack: ModsVersion | null;
  } | null>(null);
  // What the catalog recognized at the last look at the folder, so a
  // modpack's files show their titles and icons without a check.
  const [named, setNamed] = useState<{ id: string; known: ContentIdentified[] } | null>(null);
  const known = named?.id === id ? named.known : [];
  const packOffer = checked?.revision === revision ? checked.pack : null;
  // What the mods' jars say, read after the list: it never holds the list up.
  const [jars, setJars] = useState<({ id: string } & ModsFolderCheck) | null>(null);
  const jarsAsked = useRef(0);
  const read = jars?.id === id ? jars : null;
  const [checking, setChecking] = useState(false);
  const [batch, setBatch] = useState<Batch | null>(null);
  const [updatingPack, setUpdatingPack] = useState<ModsVersion | null>(null);
  const [exporting, setExporting] = useState(false);
  const [publishing, setPublishing] = useState(false);
  // Files the catalog has since marked as malware (D-066, stage 6).
  const [harmful, setHarmful] = useState<string[]>([]);
  const folder = useCommand((requestId: string) => ipc.openInstanceFolder(id, requestId, 'mods'));
  const checkButton = useRef<HTMLButtonElement>(null);
  const root = useRef<HTMLDivElement>(null);
  // Counts the lists read again after a change made from this tab: a row or a warning
  // that had the focus may have gone with them.
  const [landed, setLanded] = useState(0);
  const pack = view?.pack ?? null;

  const load = useCallback(
    (refocus = false) => {
      const arrived = () => {
        if (refocus) setLanded((n) => n + 1);
      };
      void ipc
        .modsContent(id)
        .then((found) => {
          setView(found);
          arrived();
        })
        .catch((e: unknown) => {
          setError(describeIpcError(e));
        });
      // Offline or unanswered: nothing to warn about yet.
      void ipc
        .modsHarmful(id)
        .then((found) => {
          setHarmful(found);
          arrived();
        })
        .catch(() => undefined);
      // Offline or unanswered: the files keep their names.
      void ipc
        .modsIdentify(id)
        .then((known) => {
          setNamed({ id, known });
        })
        .catch(() => undefined);
      // Only the last answer counts: a toggle asks again before the one before is back.
      const asked = ++jarsAsked.current;
      void ipc
        .modsFolderCheck(id)
        .then((found) => {
          if (asked !== jarsAsked.current || !found) return;
          setJars({ id, problems: found.problems, versions: found.versions });
        })
        .catch(() => undefined);
    },
    [id],
  );

  useEffect(() => {
    load();
  }, [load, revision]);

  const change = (action: Promise<void>) => {
    // Only a change pressed from here hands the focus on after it (a mouse press in
    // WebKit leaves the focus on the page: it is not moved, nor the page scrolled).
    const fromHere = root.current?.contains(document.activeElement) === true;
    setWorking(true);
    setError(null);
    setBatch(null);
    action
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setWorking(false);
        load(fromHere);
      });
  };

  // The newest version of the pack for this Minecraft version, when it is
  // not the one installed; the pack's own files are updated with it.
  const newerPack = async (): Promise<ModsVersion | null> => {
    if (pack === null) return null;
    try {
      const found = await ipc.modsVersions(pack.projectId, instance.versionId, null);
      const newest = found.find((v) => v.files.some((f) => packArchive(f.name)));
      return newest !== undefined && newest.id !== pack.versionId ? newest : null;
    } catch {
      return null;
    }
  };

  const check = () => {
    setChecking(true);
    setError(null);
    Promise.all([ipc.modsUpdates(id), ipc.modsIdentify(id), newerPack()])
      .then(([offers, identified, newer]) => {
        // What the pack brought is updated with the pack.
        const own = new Set((view?.items ?? []).filter((i) => i.fromPack).map((i) => i.path));
        setChecked({
          revision,
          updates: offers.filter((o) => !own.has(o.path)),
          pack: newer,
        });
        setNamed({ id, known: identified });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setChecking(false);
      });
  };

  // One after another; one that cannot go (something missing or in the way)
  // is counted and left for the player to open.
  const updateAll = async (offers: ContentUpdate[]) => {
    setBatch({ done: 0, total: offers.length, failed: 0 });
    let failed = 0;
    for (const [index, offer] of offers.entries()) {
      const world = /^saves\/([^/]+)\/datapacks\//.exec(offer.path)?.[1] ?? null;
      const payload = {
        instanceId: id,
        version: offer.toVersionId,
        world,
        replaces: offer.path,
      };
      try {
        const plan = await ipc.modsPlan(payload);
        // A download a metered connection asks about is left for the player.
        const ready =
          plan.missing.length === 0 && plan.conflicts.length === 0 && plan.approval === null;
        if (ready) await ipc.modsInstall(payload);
        else failed += 1;
      } catch {
        failed += 1;
      }
      setBatch({ done: index + 1, total: offers.length, failed });
    }
    // The offers were for the files as they were: the check after it says what is left.
    setChecked(null);
    load();
    check();
  };

  const updatingAll = batch !== null && batch.done < batch.total;
  // Something else holds the installation (the game runs, it installs): nothing here
  // changes, and the controls say so. A change made here holds them too, but they keep
  // the focus while it goes.
  const waiting = instance.busy;
  const own = working || updatingAll;
  const items = view?.items ?? [];
  // Each offer for a file still there, at its path now.
  const offered = new Map(
    (checked?.revision === revision ? checked.updates : []).map((u) => [onPath(u.path), u]),
  );
  const updates =
    checked?.revision === revision
      ? items.flatMap((item) => {
          const offer = offered.get(onPath(item.path));
          return offer === undefined ? [] : [{ ...offer, path: item.path }];
        })
      : null;
  const byPath = new Map((updates ?? []).map((u) => [u.path, u]));
  const knownByPath = new Map(known.map((k) => [onPath(k.path), k]));
  const ownVersions = new Map((read?.versions ?? []).map((v) => [onPath(v.path), v.version]));
  // One line at the toolbar's end says what the last check or "Update all" found. It
  // stays the same element, so each new answer is read out.
  const batchText =
    batch === null
      ? null
      : updatingAll
        ? t.updating({ done: batch.done, total: batch.total })
        : batch.failed > 0
          ? t.updatedSome({ failed: batch.failed })
          : t.updatedAll;
  const offersText =
    updates === null || updatingAll || checking
      ? null
      : [
          updates.length > 0
            ? t.updatesFound({ count: updates.length })
            : packOffer === null
              ? t.upToDate
              : null,
          packOffer !== null ? t.packOffered({ version: isolate(packOffer.number) }) : null,
        ]
          .filter((part) => part !== null)
          .join(' ');
  const status = [batchText, offersText].filter((part) => part !== null && part !== '').join(' ');
  // What had the focus went (a removed row or warning, Update all once nothing is left
  // to update): it goes to Check for updates.
  useFocusBack(landed, checkButton);
  useFocusBack(checked?.revision === revision ? checked.updates : null, checkButton);
  const rowNames = new Map<string, number>();
  for (const item of items) rowNames.set(rowName(item), (rowNames.get(rowName(item)) ?? 0) + 1);
  // Two files that share a name (one of them put there by hand) keep their own rows.
  const rowKey = (item: ContentItem) =>
    (rowNames.get(rowName(item)) ?? 0) > 1 ? item.path : rowName(item);
  return (
    <div ref={root} className={styles.installed}>
      {instance.busy && <Notice tone="info">{t.busy}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {harmful.length > 0 && (
        <Notice tone="danger">
          <p>
            <strong>{t.harmfulTitle}</strong>
          </p>
          <p>{t.harmfulText}</p>
          <ul className={styles.contentList}>
            {harmful.map((path) => (
              <li key={path} className={styles.contentRow}>
                <span className={styles.cardBody}>
                  <bdi dir="ltr">{path}</bdi>
                </span>
                <Button
                  size="small"
                  disabled={waiting}
                  aria-disabled={own || undefined}
                  onClick={() => {
                    if (!own) change(ipc.modsRemove(id, path));
                  }}
                >
                  {t.remove}
                </Button>
              </li>
            ))}
          </ul>
        </Notice>
      )}
      {read !== null && <Problems problems={read.problems} />}
      {pack !== null && (
        <div className={styles.packCard}>
          <ProjectIcon
            hit={{ icon: pack.icon, title: pack.title, slug: pack.title }}
            className={styles.packIcon}
          />
          <span className={styles.itemName}>
            <span>
              {t.pack({ title: isolate(pack.title), version: isolate(pack.versionNumber) })}
            </span>
            {packOffer !== null && (
              <Tag tone="warning">{t.updateTo({ version: isolate(packOffer.number) })}</Tag>
            )}
          </span>
          <span className={styles.itemMeta}>
            {t.packHint}
            {updates !== null && packOffer === null && ` ${t.packLatest}`}
          </span>
          <span className={styles.itemActions}>
            {packOffer !== null && (
              <Button
                size="small"
                disabled={waiting}
                aria-disabled={own || undefined}
                onClick={() => {
                  if (!own) setUpdatingPack(packOffer);
                }}
              >
                {t.packUpdate({ version: isolate(packOffer.number) })}
              </Button>
            )}
            <Button
              size="small"
              onClick={() => {
                onOpen(pack.projectId);
              }}
            >
              {t.open}
            </Button>
          </span>
        </div>
      )}
      <div className={styles.toolbar}>
        <Button
          ref={checkButton}
          size="small"
          disabled={waiting}
          aria-disabled={checking || own || undefined}
          onClick={() => {
            if (checking || own) return;
            setBatch(null);
            check();
          }}
        >
          <Icon name="refresh" size={18} />
          {checking ? t.checking : t.check}
        </Button>
        {updates !== null && updates.length > 1 && (
          <Button
            variant="primary"
            size="small"
            disabled={waiting}
            aria-disabled={own || checking || undefined}
            onClick={() => {
              if (!own && !checking) void updateAll(updates);
            }}
          >
            {t.updateAll({ count: updates.length })}
          </Button>
        )}
        <Button
          size="small"
          aria-disabled={folder.pending || undefined}
          onClick={() => {
            if (!folder.pending) void folder.run();
          }}
        >
          <Icon name="folder" size={18} />
          {t.openFolder}
        </Button>
        <Button
          size="small"
          disabled={waiting}
          aria-disabled={own || undefined}
          onClick={() => {
            if (!own) setExporting(true);
          }}
        >
          {t.export}
        </Button>
        <Button
          size="small"
          disabled={waiting}
          aria-disabled={own || undefined}
          onClick={() => {
            if (!own) setPublishing(true);
          }}
        >
          {t.publish}
        </Button>
        <p role="status" className={styles.toolbarStatus}>
          {status}
        </p>
      </div>
      {folder.error !== null && <Notice tone="danger">{folder.error}</Notice>}
      {view === null ? (
        error === null && <p role="status">{strings.mods.loading}</p>
      ) : items.length === 0 ? (
        <p className={styles.muted}>{t.empty}</p>
      ) : (
        FOLDERS.map((group) => {
          const inFolder = items.filter((i) => i.folder === group);
          if (inFolder.length === 0) return null;
          return (
            <Panel key={group} title={t.folders[group]} level={3} count={inFolder.length}>
              <ul className={styles.items}>
                {inFolder.map((item) => (
                  <Row
                    key={rowKey(item)}
                    item={item}
                    update={byPath.get(item.path)}
                    known={knownByPath.get(onPath(item.path))}
                    ownVersion={ownVersions.get(onPath(item.path))}
                    waiting={waiting}
                    pending={own}
                    onOpen={onOpen}
                    onCheck={() => ipc.modsNeededBy(id, item.path)}
                    onToggle={() => {
                      change(ipc.modsSetEnabled(id, item.path, !item.enabled));
                    }}
                    onRemove={(also) => {
                      change(
                        (async () => {
                          await ipc.modsRemove(id, item.path);
                          for (const path of also) await ipc.modsRemove(id, path);
                        })(),
                      );
                    }}
                    onUpdate={(title) => {
                      const offer = byPath.get(item.path);
                      if (offer !== undefined) onUpdate(offer, title, item.world);
                    }}
                  />
                ))}
              </ul>
            </Panel>
          );
        })
      )}
      {updatingPack !== null && pack !== null && (
        <PackDialog
          title={pack.title}
          version={updatingPack}
          updating={instance}
          onClose={() => {
            setUpdatingPack(null);
          }}
          onDone={() => {
            setChecked(null);
            load();
          }}
        />
      )}
      {exporting && (
        <ExportDialog
          instance={instance}
          onClose={() => {
            setExporting(false);
          }}
        />
      )}
      {publishing && (
        <PublishDialog
          instance={instance}
          onClose={() => {
            setPublishing(false);
          }}
          onOpen={(projectId) => {
            setPublishing(false);
            onOpen(projectId);
          }}
        />
      )}
    </div>
  );
}
