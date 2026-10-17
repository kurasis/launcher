import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, Gallery, InstanceSummary, Screenshot } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { gameLabel } from '../instances/loaderLabel';
import { galleryUrl } from './galleryUrl';
import styles from './Gallery.module.css';

const t = strings.gallery;
const ALL = 'all';

function when(at: string): string {
  return new Date(at).toLocaleString(numberLocale());
}

/** A screenshot with its installation, in the order the gallery shows them. */
interface Item {
  instance: InstanceSummary;
  shot: Screenshot;
}

interface ViewerProps {
  items: Item[];
  index: number;
  onMove: (index: number) => void;
  onClose: () => void;
  onDeleted: () => void;
}

/** One screenshot at full size, with its installation and actions. */
function Viewer({ items, index, onMove, onClose, onDeleted }: ViewerProps) {
  const item = items[index];
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);
  const [confirming, setConfirming] = useState(false);
  const open = useCommand((requestId: string, id: string, file: string, reveal: boolean) =>
    ipc.openScreenshot(id, file, reveal, requestId),
  );
  const remove = useCommand((requestId: string, id: string, file: string) =>
    ipc.deleteScreenshot(id, file, requestId),
  );
  useEffect(() => {
    closeRef.current?.focus();
  }, []);
  // Keys work wherever focus is while the viewer is open (after a delete the
  // focused button is gone).
  const count = items.length;
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      const forward = document.documentElement.dir === 'rtl' ? -1 : 1;
      const delta = event.key === 'ArrowRight' ? forward : event.key === 'ArrowLeft' ? -forward : 0;
      if (event.key === 'Escape') onClose();
      if (delta !== 0 && index + delta >= 0 && index + delta < count) {
        setConfirming(false);
        onMove(index + delta);
      }
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
    };
  }, [index, count, onClose, onMove]);
  if (item === undefined) return null;
  const { instance, shot } = item;
  const step = (delta: number) => {
    const next = index + delta;
    if (next >= 0 && next < items.length) {
      setConfirming(false);
      onMove(next);
    }
  };
  const error = open.error ?? remove.error;

  // On the page body: a panel's backdrop filter would otherwise contain the
  // fixed overlay.
  return createPortal(
    <div className={styles.backdrop}>
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className={styles.viewer}>
        <header className={styles.viewerHead}>
          <div>
            <h2 id={titleId} className={styles.viewerTitle}>
              {instance.displayName}
              <span className={styles.version}>{gameLabel(instance)}</span>
            </h2>
            <p className={styles.muted}>
              <span dir="ltr">{shot.file}</span>
              {' · '}
              {t.details({ date: when(shot.takenAt), size: formatBytes(shot.bytes) })}
              {shot.width !== null &&
                shot.height !== null &&
                ` · ${t.dimensions({ width: shot.width, height: shot.height })}`}
            </p>
          </div>
          <Button ref={closeRef} onClick={onClose}>
            {t.close}
          </Button>
        </header>
        <img
          className={styles.full}
          src={galleryUrl('full', instance.id, shot)}
          alt={t.open({ date: when(shot.takenAt) })}
        />
        {error !== null && <Notice tone="danger">{error}</Notice>}
        {confirming && (
          <Notice tone="warning">
            <p>{t.confirmDelete}</p>
            <div className={forms.actions}>
              <Button
                variant="primary"
                disabled={remove.pending}
                onClick={() => {
                  void remove.run(instance.id, shot.file).then((done) => {
                    setConfirming(false);
                    if (done) onDeleted();
                  });
                }}
              >
                {t.delete}
              </Button>
              <Button
                onClick={() => {
                  setConfirming(false);
                }}
              >
                {t.cancel}
              </Button>
            </div>
          </Notice>
        )}
        <footer className={styles.viewerActions}>
          <div className={styles.actions}>
            <Button
              disabled={index === 0}
              onClick={() => {
                step(-1);
              }}
            >
              {t.previous}
            </Button>
            <span className={styles.muted}>
              {t.position({ index: index + 1, count: items.length })}
            </span>
            <Button
              disabled={index === items.length - 1}
              onClick={() => {
                step(1);
              }}
            >
              {t.next}
            </Button>
          </div>
          <div className={styles.actions}>
            <Button
              disabled={open.pending}
              onClick={() => void open.run(instance.id, shot.file, false)}
            >
              {t.openFile}
            </Button>
            <Button
              disabled={open.pending}
              onClick={() => void open.run(instance.id, shot.file, true)}
            >
              {t.reveal}
            </Button>
            <Button
              disabled={confirming}
              onClick={() => {
                setConfirming(true);
              }}
            >
              {t.delete}
            </Button>
          </div>
        </footer>
      </div>
    </div>,
    document.body,
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
  const [filter, setFilter] = useState(ALL);
  const [selected, setSelected] = useState<{ instanceId: string; file: string } | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const reload = useCallback(() => {
    ipc
      .listScreenshots()
      .then((found) => {
        setGallery(found);
        setLoadError(null);
      })
      .catch((error: unknown) => {
        setLoadError(describeIpcError(error));
      });
  }, []);
  // Reload when a game session or another operation ends: it may have added shots.
  const activeId = snapshot.activeOperation?.operationId ?? null;
  useEffect(() => {
    if (activeId === null) reload();
  }, [activeId, reload]);

  const groups = (gallery?.installations ?? []).flatMap((group) => {
    const instance = snapshot.instances.find((i) => i.id === group.instanceId);
    return instance === undefined || (filter !== ALL && filter !== instance.id)
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
  const count = gallery?.installations.reduce((n, g) => n + g.screenshots.length, 0) ?? 0;

  return (
    <Panel
      title={t.title}
      aside={
        gallery !== null &&
        count > 0 && (
          <span className={styles.muted}>
            {t.summary({ count, size: formatBytes(gallery.totalBytes) })}
          </span>
        )
      }
    >
      <p className={styles.muted}>{t.hint}</p>
      <div className={styles.controls}>
        <div className={styles.field}>
          <label htmlFor={selectId}>{t.filter}</label>
          <select
            id={selectId}
            className={forms.select}
            value={filter}
            onChange={(event) => {
              setFilter(event.target.value);
            }}
          >
            <option value={ALL}>{t.all}</option>
            {snapshot.instances.map((item) => (
              <option key={item.id} value={item.id}>
                {item.displayName} · {gameLabel(item)}
              </option>
            ))}
          </select>
        </div>
        <Button onClick={reload}>{t.refresh}</Button>
      </div>
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {message !== null && <Notice tone="info">{message}</Notice>}
      {gallery !== null && groups.length === 0 && <p>{filter === ALL ? t.none : t.noneHere}</p>}
      {groups.map(({ instance, screenshots }) => (
        <section
          key={instance.id}
          className={styles.group}
          aria-labelledby={`gallery-${instance.id}`}
        >
          <header className={styles.groupHead}>
            <h3 id={`gallery-${instance.id}`} className={styles.groupTitle}>
              {instance.displayName}
            </h3>
            <span className={styles.version}>{gameLabel(instance)}</span>
            <span className={styles.muted}>{t.count({ count: screenshots.length })}</span>
          </header>
          <ul className={styles.grid}>
            {screenshots.map((shot) => (
              <li key={shot.file}>
                <button
                  type="button"
                  className={styles.thumb}
                  aria-label={t.open({ date: when(shot.takenAt) })}
                  onClick={() => {
                    setMessage(null);
                    setSelected({ instanceId: instance.id, file: shot.file });
                  }}
                >
                  <img src={galleryUrl('thumb', instance.id, shot)} alt="" loading="lazy" />
                  <span className={styles.caption}>{when(shot.takenAt)}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {index >= 0 && (
        <Viewer
          items={items}
          index={index}
          onMove={(next) => {
            const item = items[next];
            if (item !== undefined) {
              setSelected({ instanceId: item.instance.id, file: item.shot.file });
            }
          }}
          onClose={() => {
            setSelected(null);
          }}
          onDeleted={() => {
            // Show the next screenshot, or the previous one after the last.
            const next = items[index + 1] ?? items[index - 1];
            setSelected(
              next === undefined ? null : { instanceId: next.instance.id, file: next.shot.file },
            );
            setMessage(t.deleted);
            reload();
          }}
        />
      )}
    </Panel>
  );
}
