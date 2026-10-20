import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, ModsProgress, ModsWorldPlan } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc, onModsProgress } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from './Dialog';
import styles from './Mods.module.css';

const metered = strings.metered;
const MIB = 1024 * 1024;

export interface WorldTarget {
  instance: InstanceSummary;
  versionId: string;
  title: string;
}

type Stage =
  | { step: 'planning' }
  | { step: 'plan'; plan: ModsWorldPlan | null }
  | { step: 'installing'; plan: ModsWorldPlan }
  | { step: 'done'; world: string };

/**
 * Adding a map from the catalog to an installation: it becomes a new world
 * beside the player's, never over one (D-066).
 */
export function WorldDialog({
  target,
  onClose,
  onDone,
}: {
  target: WorldTarget;
  onClose: () => void;
  onDone: () => void;
}) {
  const t = strings.mods.world;
  const add = strings.mods.add;
  const { instance } = target;
  const [stage, setStage] = useState<Stage>({ step: 'planning' });
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ModsProgress | null>(null);
  const payload = { instanceId: instance.id, version: target.versionId };

  useEffect(() => {
    ipc
      .modsWorldPlan(payload)
      .then((plan) => {
        setStage({ step: 'plan', plan });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'plan', plan: null });
      });
    // Planned once on open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(
    () =>
      onModsProgress((p) => {
        if (p.instanceId === instance.id) setProgress(p);
      }),
    [instance.id],
  );

  const install = (plan: ModsWorldPlan) => {
    setError(null);
    setProgress(null);
    setStage({ step: 'installing', plan });
    ipc
      .modsInstallWorld(payload)
      .then((world) => {
        setStage({ step: 'done', world });
        onDone();
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'plan', plan });
      });
  };

  const close = () => {
    if (stage.step === 'installing') return;
    onClose();
  };

  const plan = stage.step === 'plan' || stage.step === 'installing' ? stage.plan : null;
  const shown =
    progress?.phase === 'downloading' && progress.totalBytes > 0
      ? Math.min(100, Math.floor((progress.bytes / progress.totalBytes) * 100))
      : null;

  return (
    <Dialog title={t.title({ title: target.title, name: instance.displayName })} onClose={close}>
      {stage.step === 'planning' && <p role="status">{add.planning}</p>}

      {plan !== null && (
        <>
          <p>{t.becomes({ world: plan.name, size: formatBytes(plan.size) })}</p>
          {!plan.kept && <p className={styles.muted}>{t.fetchingHint}</p>}
          {plan.approval !== null && stage.step === 'plan' && (
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
        </>
      )}

      {stage.step === 'installing' && (
        <div role="status" className={styles.progress}>
          <p>
            {progress?.phase === 'fetching'
              ? t.fetching
              : progress?.phase === 'downloading'
                ? t.downloading
                : progress?.phase === 'unpacking'
                  ? t.unpacking
                  : add.starting}
          </p>
          {shown !== null && (
            <progress max={100} value={shown} aria-label={add.progress}>
              {shown}%
            </progress>
          )}
        </div>
      )}

      {stage.step === 'done' && <Notice tone="info">{t.done({ world: stage.world })}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}

      <div className={forms.actions}>
        {stage.step === 'installing' ? (
          <Button
            onClick={() => {
              void ipc.modsCancel(instance.id).catch(() => undefined);
            }}
          >
            {add.stop}
          </Button>
        ) : stage.step === 'done' ? (
          <Button variant="primary" onClick={onClose}>
            {add.close}
          </Button>
        ) : (
          <>
            <Button
              variant="primary"
              disabled={plan === null}
              onClick={() => {
                if (plan !== null) install(plan);
              }}
            >
              {t.install}
            </Button>
            <Button onClick={onClose}>{add.cancel}</Button>
          </>
        )}
      </div>
    </Dialog>
  );
}
