import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, ModsKind, ModsPlan, ModsProgress } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc, onModsProgress } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from '../../components/Dialog';
import styles from './Mods.module.css';
import { Problems } from './Problems';

const metered = strings.metered;
const MIB = 1024 * 1024;

export interface AddTarget {
  instance: InstanceSummary;
  versionId: string;
  title: string;
  kind: ModsKind;
  /** A file of the player's this replaces (an update). */
  replaces: string | null;
  /** The world of a data pack being updated. */
  world: string | null;
}

type Stage =
  | { step: 'world' }
  | { step: 'planning' }
  | { step: 'plan'; plan: ModsPlan }
  | { step: 'installing'; plan: ModsPlan }
  | { step: 'done'; count: number };

function percent(progress: ModsProgress | null): number | null {
  if (progress === null) return null;
  if (progress.phase === 'downloading' && progress.totalBytes > 0) {
    return Math.min(100, Math.floor((progress.bytes / progress.totalBytes) * 100));
  }
  return null;
}

/**
 * What adding a version to an installation brings (the mods it needs, what is
 * in the way), then the install with its progress (D-066).
 */
export function AddDialog({
  target,
  onClose,
  onDone,
}: {
  target: AddTarget;
  onClose: () => void;
  onDone: () => void;
}) {
  const t = strings.mods.add;
  const { instance } = target;
  const datapack = target.kind === 'datapack';
  // An update of a data pack knows its world already.
  const choosing = datapack && target.world === null;
  const [worlds, setWorlds] = useState<string[] | null>(null);
  const [world, setWorld] = useState(target.world ?? '');
  const [stage, setStage] = useState<Stage>(choosing ? { step: 'world' } : { step: 'planning' });
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ModsProgress | null>(null);
  // Another version of the project, when the plan offers one that runs here.
  const [versionId, setVersionId] = useState(target.versionId);

  useEffect(() => {
    if (!choosing) return;
    void ipc
      .modsContent(instance.id)
      .then((view) => {
        setWorlds(view.worlds);
        setWorld(view.worlds[0] ?? '');
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      });
  }, [choosing, instance.id]);

  const payloadFor = (version: string) => ({
    instanceId: instance.id,
    version,
    world: datapack ? world : null,
    replaces: target.replaces,
  });

  // State changes only when the answer comes.
  const requestPlan = (version = versionId) => {
    ipc
      .modsPlan(payloadFor(version))
      .then((found) => {
        setStage({ step: 'plan', plan: found });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage(choosing ? { step: 'world' } : { step: 'plan', plan: emptyPlan });
      });
  };

  const plan = (version = versionId) => {
    setError(null);
    setStage({ step: 'planning' });
    requestPlan(version);
  };

  useEffect(() => {
    if (!choosing) requestPlan();
    // Planned once on open; a data pack waits for its world.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(
    () =>
      onModsProgress((p) => {
        if (p.instanceId === instance.id) setProgress(p);
      }),
    [instance.id],
  );

  const install = (ready: ModsPlan) => {
    setError(null);
    setProgress(null);
    setStage({ step: 'installing', plan: ready });
    ipc
      .modsInstall(payloadFor(versionId))
      .then((count) => {
        setStage({ step: 'done', count });
        onDone();
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'plan', plan: ready });
      });
  };

  const close = () => {
    if (stage.step === 'installing') return;
    onClose();
  };

  return (
    <Dialog
      title={t.title({ title: target.title, name: instance.displayName })}
      onClose={close}
      wide
    >
      {stage.step === 'world' && (
        <>
          {worlds === null ? (
            error === null && <p role="status">{strings.mods.loading}</p>
          ) : worlds.length === 0 ? (
            <Notice tone="warning">{t.noWorlds}</Notice>
          ) : (
            <Field label={t.world} hint={t.worldHint}>
              {(props) => (
                <select
                  {...props}
                  className={forms.select}
                  value={world}
                  onChange={(event) => {
                    setWorld(event.target.value);
                  }}
                >
                  {worlds.map((w) => (
                    <option key={w} value={w}>
                      {w}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          )}
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button
              variant="primary"
              disabled={world === ''}
              onClick={() => {
                plan();
              }}
            >
              {t.next}
            </Button>
            <Button onClick={onClose}>{t.cancel}</Button>
          </div>
        </>
      )}

      {stage.step === 'planning' && <p role="status">{t.planning}</p>}

      {(stage.step === 'plan' || stage.step === 'installing') && (
        <PlanView
          plan={stage.plan}
          installing={stage.step === 'installing'}
          progress={progress}
          error={error}
          onInstall={() => {
            install(stage.plan);
          }}
          onInstead={(version) => {
            setVersionId(version);
            plan(version);
          }}
          onCancel={() => {
            void ipc.modsCancel(instance.id).catch(() => undefined);
          }}
          onClose={onClose}
        />
      )}

      {stage.step === 'done' && (
        <>
          <Notice tone="info">{t.done({ count: stage.count })}</Notice>
          <div className={forms.actions}>
            <Button variant="primary" onClick={onClose}>
              {t.close}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}

const emptyPlan: ModsPlan = {
  items: [],
  missing: [],
  conflicts: [],
  misfit: null,
  totalBytes: 0,
  approval: null,
  turnOn: [],
  problems: [],
  instead: null,
};

function PlanView({
  plan,
  installing,
  progress,
  error,
  onInstall,
  onInstead,
  onCancel,
  onClose,
}: {
  plan: ModsPlan;
  installing: boolean;
  progress: ModsProgress | null;
  error: string | null;
  onInstall: () => void;
  onInstead: (version: string) => void;
  onCancel: () => void;
  onClose: () => void;
}) {
  const t = strings.mods.add;
  const ready = plan.items.length > 0 && plan.missing.length === 0 && plan.conflicts.length === 0;
  const needed = plan.items.some((i) => !i.present) || plan.turnOn.length > 0;
  const fetching = plan.items.some((i) => !i.kept && !i.present);
  const shown = percent(progress);
  return (
    <>
      {plan.misfit !== null && <Notice tone="warning">{t.misfit[plan.misfit]}</Notice>}
      {plan.instead !== null && !installing && (
        <Notice tone="info">
          <p>{t.instead({ number: plan.instead.number })}</p>
          <Button
            onClick={() => {
              if (plan.instead !== null) onInstead(plan.instead.versionId);
            }}
          >
            {t.takeInstead({ number: plan.instead.number })}
          </Button>
        </Notice>
      )}
      {plan.items.length > 0 && (
        <ul className={styles.planList}>
          {plan.items.map((item) => (
            <li key={item.versionId} className={styles.planItem}>
              <span className={styles.cardTitle}>
                <bdi>{item.title || item.fileName}</bdi>
                <span className={styles.muted}>
                  <bdi>{item.number}</bdi>
                </span>
                {item.dependency && <StatusBadge tone="neutral" label={t.needed} />}
                {item.present && <StatusBadge tone="success" label={t.present} />}
                {!item.kept && !item.present && <StatusBadge tone="warning" label={t.fromSource} />}
              </span>
              <span className={styles.muted}>
                <bdi dir="ltr">{item.path}</bdi>
                {!item.present && ` · ${formatBytes(item.size)}`}
                {item.replaces !== null && item.replaces !== item.path && (
                  <>
                    {' · '}
                    {t.replaces({ file: item.replaces })}
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      {plan.turnOn.length > 0 && (
        <p className={styles.muted}>
          {t.turnOn({ names: plan.turnOn.map((o) => o.title).join(', ') })}
        </p>
      )}
      {plan.missing.length > 0 && (
        <Notice tone="danger">
          <p>{t.missing}</p>
          <ul>
            {plan.missing.map((m, index) => (
              <li key={m.projectId ?? index}>{m.title ?? t.unknownProject}</li>
            ))}
          </ul>
        </Notice>
      )}
      {plan.conflicts.length > 0 && (
        <Notice tone="danger">
          <ul>
            {plan.conflicts.map((c) => (
              <li key={c.path}>
                {c.reason === 'incompatible'
                  ? t.incompatible({ title: c.title ?? c.path })
                  : c.reason === 'held'
                    ? t.held({ title: c.title ?? c.path })
                    : t.nameTaken({ file: c.path })}
              </li>
            ))}
          </ul>
        </Notice>
      )}
      <Problems problems={plan.problems} />
      {ready && needed && (
        <p className={styles.muted}>
          {t.total({ size: formatBytes(plan.totalBytes) })}
          {fetching && ` ${t.fetchingHint}`}
        </p>
      )}
      {ready && !needed && <p className={styles.muted}>{t.nothingToDo}</p>}
      {ready && needed && plan.approval !== null && !installing && (
        <Notice tone="warning">
          <p>
            <strong>{metered.approvalTitle({ size: formatBytes(plan.approval.bytes) })}</strong>
          </p>
          <p>
            {plan.approval.reason === 'over_limit'
              ? metered.approvalOverLimit({
                  size: formatBytes(plan.approval.bytes),
                  used: formatBytes(plan.approval.monthBytes),
                  limit: formatBytes(plan.approval.monthlyLimitMib * MIB),
                })
              : metered.approvalLarge({
                  files: plan.approval.files,
                  size: formatBytes(plan.approval.bytes),
                })}
          </p>
        </Notice>
      )}
      {installing && (
        <div role="status" className={styles.progress}>
          <p>
            {progress?.phase === 'fetching'
              ? t.fetching({ count: progress.files })
              : progress?.phase === 'downloading'
                ? t.downloading({ files: progress.files, total: progress.totalFiles })
                : t.starting}
          </p>
          {shown !== null && (
            <progress max={100} value={shown} aria-label={t.progress}>
              {shown}%
            </progress>
          )}
        </div>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        {installing ? (
          <Button onClick={onCancel}>{t.stop}</Button>
        ) : (
          <>
            <Button variant="primary" disabled={!ready || !needed} onClick={onInstall}>
              {t.install}
            </Button>
            <Button onClick={onClose}>{t.cancel}</Button>
          </>
        )}
      </div>
    </>
  );
}
