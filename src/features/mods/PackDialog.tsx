import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  ModsPack,
  ModsPackReport,
  ModsProgress,
  ModsVersion,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc, onModsProgress } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from './Dialog';
import styles from './Mods.module.css';

const metered = strings.metered;
const MIB = 1024 * 1024;
/** How often the catalog is asked while it prepares a pack. */
const POLL_MS = 3000;
const MAX_NAME = 64;

const FAILURES = [
  'bad_pack',
  'unsupported_pack',
  'unsupported_host',
  'too_big',
  'hash_mismatch',
] as const;

/** Why the catalog could not prepare a pack, in the player's words. */
function packFailure(error: string | null): string {
  const t = strings.mods.pack.failures;
  return (FAILURES as readonly (string | null)[]).includes(error)
    ? t[error as (typeof FAILURES)[number]]
    : t.other;
}

type Stage =
  | { step: 'preparing' }
  | { step: 'ready' }
  | { step: 'creating' }
  | { step: 'installing'; instance: InstanceSummary }
  | { step: 'stopped'; instance: InstanceSummary }
  | { step: 'done'; instance: InstanceSummary; report: ModsPackReport };

function percent(progress: ModsProgress | null): number | null {
  if (progress === null) return null;
  if (progress.phase === 'downloading' && progress.totalBytes > 0) {
    return Math.min(100, Math.floor((progress.bytes / progress.totalBytes) * 100));
  }
  if (progress.phase === 'unpacking' && progress.totalFiles > 0) {
    return Math.min(100, Math.floor((progress.files / progress.totalFiles) * 100));
  }
  return null;
}

/**
 * Installing a modpack as a new installation (D-066 stage 3): the catalog
 * prepares the pack's files, the player names the installation, and the
 * launcher creates it and puts the files in. With `updating`, the version
 * goes into that installation made from the same pack, after a backup.
 */
export function PackDialog({
  title,
  version,
  updating = null,
  onClose,
  onDone,
}: {
  title: string;
  version: ModsVersion;
  updating?: InstanceSummary | null;
  onClose: () => void;
  onDone: (instanceId: string) => void;
}) {
  const t = strings.mods.pack;
  const nameId = useId();
  const [pack, setPack] = useState<ModsPack | null>(null);
  const [stage, setStage] = useState<Stage>({ step: 'preparing' });
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState(`${title} ${version.number}`.trim().slice(0, MAX_NAME));
  const [progress, setProgress] = useState<ModsProgress | null>(null);
  const instanceId =
    stage.step === 'installing' || stage.step === 'stopped' || stage.step === 'done'
      ? stage.instance.id
      : null;
  // One installation however often the button is pressed.
  const createRequest = useRef(crypto.randomUUID());

  // Asked once to prepare, then looked at until it is ready or failed.
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const look = (prepare: boolean) => {
      ipc
        .modsPack(version.id, prepare)
        .then((found) => {
          if (!live) return;
          setPack(found);
          if (found.state === 'ready') {
            setStage({ step: 'ready' });
          } else if (found.state !== 'failed') {
            timer = setTimeout(() => {
              look(false);
            }, POLL_MS);
          }
        })
        .catch((e: unknown) => {
          if (live) setError(describeIpcError(e));
        });
    };
    look(true);
    return () => {
      live = false;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [version.id]);

  useEffect(() => {
    if (instanceId === null) return undefined;
    return onModsProgress((p) => {
      if (p.instanceId === instanceId) setProgress(p);
    });
  }, [instanceId]);

  const apply = (instance: InstanceSummary) => {
    setError(null);
    setProgress(null);
    setStage({ step: 'installing', instance });
    const payload = { instanceId: instance.id, version: version.id };
    (updating !== null ? ipc.modsUpdatePack(payload) : ipc.modsApplyPack(payload))
      .then((report) => {
        setStage({ step: 'done', instance, report });
        onDone(instance.id);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'stopped', instance });
      });
  };

  const install = () => {
    setError(null);
    setStage({ step: 'creating' });
    ipc
      .modsCreatePackInstance(
        { version: version.id, displayName: name.trim() },
        createRequest.current,
      )
      .then(apply)
      .catch((e: unknown) => {
        // A recorded failure would be replayed for the same request.
        createRequest.current = crypto.randomUUID();
        setError(describeIpcError(e));
        setStage({ step: 'ready' });
      });
  };

  const busy = stage.step === 'creating' || stage.step === 'installing';
  const close = () => {
    if (!busy) onClose();
  };
  const shown = percent(progress);
  const loaders = strings.loaders.names;
  const nameOk = name.trim().length > 0 && name.trim().length <= MAX_NAME;

  return (
    <Dialog
      title={updating !== null ? t.updateTitle({ title }) : t.title({ title })}
      onClose={close}
      wide
    >
      {stage.step === 'preparing' &&
        (pack?.state === 'failed' ? (
          <Notice tone="danger">{packFailure(pack.error)}</Notice>
        ) : (
          error === null && (
            <p role="status">
              {pack !== null && pack.total > 0
                ? t.fetching({ kept: pack.kept, total: pack.total })
                : t.preparing}
            </p>
          )
        ))}

      {stage.step !== 'preparing' && pack !== null && stage.step !== 'done' && (
        <>
          <p>
            {strings.loaders.game({ version: pack.game ?? '' })}
            {' · '}
            {pack.loader === null
              ? t.noLoader
              : `${loaders[pack.loader.kind]} ${pack.loader.version}`}
            {' · '}
            {t.mods({ count: pack.mods })}
          </p>
          <p className={styles.muted}>
            {t.size({ size: formatBytes(pack.totalBytes) })}
            {pack.skipped > 0 && ` ${t.skipped({ count: pack.skipped })}`}
          </p>
        </>
      )}

      {stage.step === 'ready' && pack !== null && (
        <>
          {updating !== null ? (
            <Notice tone="info">{t.updateNote}</Notice>
          ) : (
            <div className={styles.field}>
              <label htmlFor={nameId}>{t.name}</label>
              <input
                id={nameId}
                className={forms.input}
                value={name}
                maxLength={MAX_NAME}
                onChange={(event) => {
                  setName(event.target.value);
                }}
              />
            </div>
          )}
          {pack.approval !== null && (
            <Notice tone="warning">
              <p>
                <strong>{metered.approvalTitle({ size: formatBytes(pack.approval.bytes) })}</strong>
              </p>
              <p>
                {pack.approval.reason === 'over_limit'
                  ? metered.approvalOverLimit({
                      size: formatBytes(pack.approval.bytes),
                      used: formatBytes(pack.approval.monthBytes),
                      limit: formatBytes(pack.approval.monthlyLimitMib * MIB),
                    })
                  : metered.approvalLarge({
                      files: pack.approval.files,
                      size: formatBytes(pack.approval.bytes),
                    })}
              </p>
            </Notice>
          )}
        </>
      )}

      {stage.step === 'creating' && <p role="status">{t.creating}</p>}

      {stage.step === 'installing' && (
        <div role="status" className={styles.progress}>
          <p>
            {progress?.phase === 'downloading'
              ? strings.mods.add.downloading({ files: progress.files, total: progress.totalFiles })
              : progress?.phase === 'unpacking'
                ? t.unpacking({ files: progress.files, total: progress.totalFiles })
                : strings.mods.add.starting}
          </p>
          {shown !== null && (
            <progress max={100} value={shown} aria-label={strings.mods.add.progress}>
              {shown}%
            </progress>
          )}
        </div>
      )}

      {stage.step === 'stopped' && (
        <Notice tone="info">{updating !== null ? t.updateStopped : t.stopped}</Notice>
      )}

      {stage.step === 'done' && (
        <Notice tone="info">
          <p>
            {updating !== null
              ? t.updated({ version: version.number })
              : t.done({ name: stage.instance.displayName })}
          </p>
          {stage.report.keptYours > 0 && <p>{t.keptYours({ count: stage.report.keptYours })}</p>}
          {stage.report.leftOut > 0 && <p>{t.leftOut({ count: stage.report.leftOut })}</p>}
        </Notice>
      )}

      {error !== null && <Notice tone="danger">{error}</Notice>}

      <div className={forms.actions}>
        {stage.step === 'ready' &&
          (updating !== null ? (
            <Button
              variant="primary"
              onClick={() => {
                apply(updating);
              }}
            >
              {t.updateStart}
            </Button>
          ) : (
            <Button variant="primary" disabled={!nameOk} onClick={install}>
              {t.start}
            </Button>
          ))}
        {stage.step === 'installing' && (
          <Button
            onClick={() => {
              void ipc.modsCancel(stage.instance.id).catch(() => undefined);
            }}
          >
            {strings.mods.add.stop}
          </Button>
        )}
        {stage.step === 'stopped' && (
          <Button
            variant="primary"
            onClick={() => {
              apply(stage.instance);
            }}
          >
            {t.retry}
          </Button>
        )}
        {!busy && (
          <Button variant={stage.step === 'done' ? 'primary' : 'secondary'} onClick={onClose}>
            {stage.step === 'done' ? t.close : strings.mods.add.cancel}
          </Button>
        )}
      </div>
    </Dialog>
  );
}
