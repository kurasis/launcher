import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import { flushSync } from 'react-dom';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Dialog } from '../../components/Dialog';
import { coveredByDialog } from '../../components/dialogStack';
import { Icon } from '../../components/Icon';
import { LiveStatus } from '../../components/LiveStatus';
import { Notice } from '../../components/Notice';
import { Screen } from '../../components/Screen';
import { useFocusBack } from '../../components/useFocusBack';
import a11y from '../../components/a11y.module.css';
import type { AppSnapshot, Gallery, InstanceSummary, Screenshot } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { focusLost } from '../../lib/focus';
import { describeIpcError, ipc } from '../../lib/ipc';
import { isolate, numberLocale, strings } from '../../lib/strings';
import { shortWhen } from '../../lib/time';
import { useCommand } from '../../lib/useCommand';
import { InstallationSelect } from '../instances/InstallationSelect';
import { gameLabel } from '../instances/loaderLabel';
import { galleryUrl } from './galleryUrl';
import styles from './Gallery.module.css';

const t = strings.gallery;

/** The full date and time: what tells two screenshots of one day apart. */
function when(at: string): string {
  return new Date(at).toLocaleString(numberLocale());
}

/** "Survival, Minecraft 1.21.1": a group's heading and the viewer's title. */
const groupName = (instance: InstanceSummary) =>
  t.group({ name: isolate(instance.displayName), game: isolate(gameLabel(instance)) });

/** A screenshot with its installation, in the order the gallery shows them. */
interface Item {
  instance: InstanceSummary;
  shot: Screenshot;
}

interface Shown {
  instanceId: string;
  file: string;
}

/** Arrow keys typed into a field elsewhere (the Ctrl K palette) move its caret, not the shots. */
function typing(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || target.closest('input, textarea, select') !== null)
  );
}

interface ViewerProps {
  items: Item[];
  index: number;
  /** What the last delete did, said in the viewer while it stays open. */
  message: string | null;
  /** What takes the focus back on closing: the thumbnail of the shot shown last. */
  returnFocus: RefObject<HTMLElement | null>;
  /** A delete goes (the gallery's, so it outlives the viewer). */
  deleting: boolean;
  deleteError: string | null;
  onMove: (index: number) => void;
  onClose: () => void;
  /** Deletes the shot; the gallery then shows the next one. */
  onDelete: (item: Item) => Promise<unknown>;
}

/** One screenshot at full size, with its installation and actions. */
function Viewer({
  items,
  index,
  message,
  returnFocus,
  deleting,
  deleteError,
  onMove,
  onClose,
  onDelete,
}: ViewerProps) {
  const item = items[index];
  const closeRef = useRef<HTMLButtonElement>(null);
  const deleteRef = useRef<HTMLButtonElement>(null);
  const figureRef = useRef<HTMLElement>(null);
  const [confirming, setConfirming] = useState(false);
  const open = useCommand((requestId: string, id: string, file: string, reveal: boolean) =>
    ipc.openScreenshot(id, file, reveal, requestId),
  );
  // The question closes after its answer or Cancel: the focus it had goes back to Delete
  // at once, before the dialog would take it to its first control.
  const closeQuestion = () => {
    const at = document.activeElement;
    const had = focusLost() || at?.closest('[role="alertdialog"]') != null;
    flushSync(() => {
      setConfirming(false);
    });
    if (had) (deleteRef.current ?? closeRef.current)?.focus();
  };
  const count = items.length;
  // While a delete goes, the viewer stays on its shot: what follows it is shown next.
  const step = (delta: number) => {
    const next = index + delta;
    if (deleting || next < 0 || next >= count) return;
    setConfirming(false);
    // What the other shot's buttons did is not this one's (reset only clears what ended).
    if (!open.pending) open.reset();
    onMove(next);
  };
  const stepRef = useRef(step);
  useLayoutEffect(() => {
    stepRef.current = step;
  });
  // The arrow keys work wherever the focus is in the viewer (after a delete the focused
  // button is gone); Escape is the dialog's.
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      // A dialog over the viewer (a download question) or a field takes them.
      if (event.defaultPrevented || coveredByDialog(figureRef.current) || typing(event.target)) {
        return;
      }
      event.preventDefault();
      // The next shot is to the right, or to the left in Arabic.
      const rtl = document.documentElement.dir === 'rtl';
      stepRef.current((event.key === 'ArrowRight') !== rtl ? 1 : -1);
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
    };
  }, []);
  if (item === undefined) return null;
  const { instance, shot } = item;
  const error = open.error ?? deleteError;
  const first = index === 0;
  const last = index === count - 1;

  return (
    <Dialog
      title={groupName(instance)}
      onClose={onClose}
      full
      initialFocus={closeRef}
      returnFocus={returnFocus}
    >
      <p className={styles.meta}>
        <bdi dir="ltr">{shot.file}</bdi>
        {' · '}
        {t.details({ date: isolate(when(shot.takenAt)), size: isolate(formatBytes(shot.bytes)) })}
        {shot.width !== null &&
          shot.height !== null &&
          ` · ${isolate(t.dimensions({ width: shot.width, height: shot.height }))}`}
      </p>
      <figure
        ref={figureRef}
        className={[styles.figure, confirming && styles.asking].filter(Boolean).join(' ')}
      >
        <img
          className={styles.full}
          src={galleryUrl('full', instance.id, shot)}
          alt={t.open({ date: when(shot.takenAt) })}
        />
      </figure>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {confirming && (
        <Confirm
          label={shot.file}
          text={t.confirmDelete}
          action={t.delete}
          cancel={t.cancel}
          pending={deleting}
          onYes={() => {
            if (deleting) return;
            if (!open.pending) open.reset();
            void onDelete(item).then(closeQuestion);
          }}
          onNo={closeQuestion}
        />
      )}
      <footer className={styles.viewerActions}>
        <div className={styles.actions}>
          {/* At the ends they stay focusable: pressed to the end, a button keeps the focus. */}
          <Button
            size="small"
            className={styles.end}
            aria-disabled={first || deleting || undefined}
            onClick={() => {
              step(-1);
            }}
          >
            <Icon name="prev" size={18} mirror />
            {t.previous}
          </Button>
          <span className={styles.position}>{t.position({ index: index + 1, count })}</span>
          <Button
            size="small"
            className={styles.end}
            aria-disabled={last || deleting || undefined}
            onClick={() => {
              step(1);
            }}
          >
            {t.next}
            <Icon name="next" size={18} mirror />
          </Button>
        </div>
        <div className={styles.actions}>
          <Button
            size="small"
            aria-disabled={open.pending || undefined}
            onClick={() => {
              if (!open.pending) void open.run(instance.id, shot.file, false);
            }}
          >
            <Icon name="image" size={18} />
            {t.openFile}
          </Button>
          <Button
            size="small"
            aria-disabled={open.pending || undefined}
            onClick={() => {
              if (!open.pending) void open.run(instance.id, shot.file, true);
            }}
          >
            <Icon name="folder" size={18} />
            {t.reveal}
          </Button>
          {!confirming && (
            <Button
              ref={deleteRef}
              size="small"
              variant="danger"
              onClick={() => {
                setConfirming(true);
              }}
            >
              <Icon name="trash" size={18} />
              {t.delete}
            </Button>
          )}
          <Button ref={closeRef} size="small" onClick={onClose}>
            {t.close}
          </Button>
        </div>
      </footer>
      <LiveStatus>{message}</LiveStatus>
    </Dialog>
  );
}

/**
 * Screenshots of every installation (D-050), grouped by installation with its
 * game and loader version, newest first; a viewer shows one at full size.
 */
export function GalleryView({ snapshot }: { snapshot: AppSnapshot }) {
  const selectId = useId();
  const [gallery, setGallery] = useState<Gallery | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Refresh was pressed and its answer has not come.
  const [refreshing, setRefreshing] = useState(false);
  const [filter, setFilter] = useState<string | null>(null);
  // The shot shown last, kept after the viewer closes: its thumbnail takes the focus back.
  const [selected, setSelected] = useState<Shown | null>(null);
  const [viewing, setViewing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  // Counts reloads after a delete: the focus on a thumbnail that went is handed on.
  const [landed, setLanded] = useState(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const lastThumb = useRef<HTMLButtonElement>(null);
  // The viewer gives the focus back to the shot shown last, or to the title when it
  // closed with the last one deleted.
  const returnFocus = useMemo<RefObject<HTMLElement | null>>(
    () => ({
      get current() {
        return lastThumb.current ?? heading.current;
      },
    }),
    [],
  );
  // Only the newest answer counts: an older one may arrive after it.
  const asked = useRef(0);
  const remove = useCommand((requestId: string, id: string, file: string) =>
    ipc.deleteScreenshot(id, file, requestId),
  );

  const reload = useCallback(() => {
    const ask = ++asked.current;
    return ipc
      .listScreenshots()
      .then((found) => {
        if (ask !== asked.current) return;
        setGallery(found);
        setLoadError(null);
      })
      .catch((error: unknown) => {
        if (ask === asked.current) setLoadError(describeIpcError(error));
      });
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);
  // Reload when a game session or another operation ends: it may have added shots.
  const activeId = snapshot.activeOperation?.operationId ?? null;
  const wasActive = useRef(activeId);
  useEffect(() => {
    const was = wasActive.current;
    wasActive.current = activeId;
    if (was !== null && activeId === null) void reload();
  }, [activeId, reload]);
  useFocusBack(landed, lastThumb, heading);

  const groups = (gallery?.installations ?? []).flatMap((group) => {
    const instance = snapshot.instances.find((i) => i.id === group.instanceId);
    return instance === undefined || (filter !== null && filter !== instance.id)
      ? []
      : [{ instance, screenshots: group.screenshots }];
  });
  const items: Item[] = groups.flatMap((g) =>
    g.screenshots.map((shot) => ({ instance: g.instance, shot })),
  );
  const index =
    selected === null
      ? -1
      : items.findIndex(
          (i) => i.instance.id === selected.instanceId && i.shot.file === selected.file,
        );
  const open = viewing && index >= 0;
  // What is shown: the filter's installation, or all of them.
  const size = items.reduce((total, item) => total + item.shot.bytes, 0);
  const show = (item: Item | undefined) => {
    setSelected(item === undefined ? null : { instanceId: item.instance.id, file: item.shot.file });
  };
  // What the delete finds when it ends: the viewer may have moved, closed or opened again.
  const now = useRef({ items, selected });
  useLayoutEffect(() => {
    now.current = { items, selected };
  });
  const deleteShot = async (item: Item) => {
    // Emptied first, so the same words are read out again.
    setMessage(null);
    const done = await remove.run(item.instance.id, item.shot.file);
    if (done === undefined) return;
    const { items: shown, selected: current } = now.current;
    const same = (other: Item | Shown) =>
      ('instance' in other ? other.instance.id : other.instanceId) === item.instance.id &&
      ('shot' in other ? other.shot.file : other.file) === item.shot.file;
    // The viewer moves on only from the shot deleted: to the next, or the previous after
    // the last; with the last one it closes.
    if (current !== null && same(current)) {
      const at = shown.findIndex(same);
      const next = shown[at + 1] ?? shown[at - 1];
      show(next);
      if (next === undefined) setViewing(false);
    }
    setMessage(t.deleted);
    void reload().then(() => {
      setLanded((n) => n + 1);
    });
  };

  return (
    <Screen title={t.title} headingRef={heading}>
      <div className={styles.toolbar}>
        <p className={styles.hint}>{t.hint}</p>
        <InstallationSelect
          id={selectId}
          label={t.filter}
          none={t.all}
          instances={snapshot.instances}
          value={filter}
          onChange={setFilter}
          fit
        />
        <Button
          size="small"
          aria-disabled={refreshing || undefined}
          onClick={() => {
            if (refreshing) return;
            setRefreshing(true);
            void reload().finally(() => {
              setRefreshing(false);
            });
          }}
        >
          <Icon name="refresh" size={18} />
          {t.refresh}
        </Button>
        {items.length > 0 && (
          <span className={styles.count}>
            {t.summary({ count: items.length, size: isolate(formatBytes(size)) })}
          </span>
        )}
      </div>
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {remove.error !== null && !open && <Notice tone="danger">{remove.error}</Notice>}
      {/* What a delete did, here once the viewer is closed. */}
      <LiveStatus>{open ? null : message}</LiveStatus>
      {gallery === null && loadError === null && <p className={styles.hint}>{t.loading}</p>}
      {gallery !== null && groups.length === 0 && (
        <p className={styles.hint}>{filter === null ? t.none : t.noneHere}</p>
      )}
      {groups.map(({ instance, screenshots }) => (
        <section
          key={instance.id}
          className={styles.group}
          aria-labelledby={`gallery-${instance.id}`}
        >
          <h3 className={styles.groupTitle}>
            <span id={`gallery-${instance.id}`}>{groupName(instance)}</span>{' '}
            <span className={styles.groupCount} aria-hidden="true">
              {screenshots.length.toLocaleString(numberLocale())}
            </span>
            <span className={a11y.visuallyHidden}>{t.count({ count: screenshots.length })}</span>
          </h3>
          <ul className={styles.grid}>
            {screenshots.map((shot) => (
              <li key={shot.file} className={styles.shot}>
                <button
                  ref={
                    selected?.instanceId === instance.id && selected.file === shot.file
                      ? lastThumb
                      : undefined
                  }
                  type="button"
                  className={styles.thumb}
                  aria-label={t.open({ date: when(shot.takenAt) })}
                  onClick={() => {
                    setMessage(null);
                    if (!remove.pending) remove.reset();
                    setSelected({ instanceId: instance.id, file: shot.file });
                    setViewing(true);
                  }}
                >
                  <img src={galleryUrl('thumb', instance.id, shot)} alt="" loading="lazy" />
                </button>
                <p className={styles.caption}>
                  {t.caption({
                    date: isolate(shortWhen(shot.takenAt, { capital: true })),
                    size: isolate(formatBytes(shot.bytes)),
                  })}
                </p>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {open && (
        <Viewer
          items={items}
          index={index}
          message={message}
          returnFocus={returnFocus}
          deleting={remove.pending}
          deleteError={remove.error}
          onMove={(next) => {
            setMessage(null);
            if (!remove.pending) remove.reset();
            show(items[next]);
          }}
          onClose={() => {
            // Said in the viewer already: the page does not say it again.
            setMessage(null);
            setViewing(false);
          }}
          onDelete={deleteShot}
        />
      )}
    </Screen>
  );
}
