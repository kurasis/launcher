import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  LoaderKind,
  ModsImportFound,
  ModsImportReport,
  ModsProgress,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc, onModsProgress } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { MOD_LOADER_KINDS } from '../instances/loaderLabel';
import { Dialog } from '../../components/Dialog';
import styles from './Mods.module.css';

type Stage =
  | { step: 'choose' }
  | { step: 'reading' }
  | { step: 'review'; found: ModsImportFound }
  | { step: 'creating'; found: ModsImportFound }
  | { step: 'copying'; instance: InstanceSummary }
  | { step: 'stopped'; instance: InstanceSummary }
  | { step: 'done'; instance: InstanceSummary; report: ModsImportReport };

/**
 * Bringing in an installation from Prism Launcher, MultiMC or a folder of
 * mods (D-066 stage 5): pick it, check the version and loader, and its files
 * are copied into a new installation.
 */
export function ImportDialog({
  onClose,
  onDone,
}: {
  onClose: () => void;
  /** The new installation, once its files are in. */
  onDone: (instanceId: string) => void;
}) {
  const t = strings.mods.bringIn;
  const [stage, setStage] = useState<Stage>({ step: 'choose' });
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ModsProgress | null>(null);
  const [name, setName] = useState('');
  const [game, setGame] = useState('');
  const [loader, setLoader] = useState<LoaderKind | 'none'>('none');
  const [games, setGames] = useState<string[]>([]);

  useEffect(() => {
    void ipc
      .listSupportedVersions(false)
      .then((catalog) => {
        setGames(
          catalog.versions
            .filter((v) => v.versionType === 'release' && v.special == null)
            .map((v) => v.id),
        );
      })
      .catch(() => undefined);
  }, []);

  const instanceId =
    stage.step === 'copying' || stage.step === 'stopped' || stage.step === 'done'
      ? stage.instance.id
      : null;
  useEffect(
    () =>
      onModsProgress((p) => {
        if (p.instanceId === instanceId) setProgress(p);
      }),
    [instanceId],
  );

  const pick = (folder: boolean) => {
    setError(null);
    setStage({ step: 'reading' });
    ipc
      .modsImportPick(folder, folder ? t.pickFolderTitle : t.pickZipTitle)
      .then((found) => {
        if (found === null) {
          setStage({ step: 'choose' });
          return;
        }
        setName(found.name);
        setGame(found.game ?? '');
        setLoader(found.loader ?? 'none');
        setStage({ step: 'review', found });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'choose' });
      });
  };

  const copy = (instance: InstanceSummary) => {
    setError(null);
    setProgress(null);
    setStage({ step: 'copying', instance });
    ipc
      .modsImportApply(instance.id)
      .then((report) => {
        setStage({ step: 'done', instance, report });
        onDone(instance.id);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'stopped', instance });
      });
  };

  const create = (found: ModsImportFound) => {
    setError(null);
    setStage({ step: 'creating', found });
    ipc
      .modsImportCreate({
        displayName: name.trim(),
        gameVersion: game,
        loader: loader === 'none' ? null : loader,
      })
      .then(copy)
      .catch((e: unknown) => {
        setError(describeIpcError(e));
        setStage({ step: 'review', found });
      });
  };

  const busy = stage.step === 'reading' || stage.step === 'creating' || stage.step === 'copying';
  const close = () => {
    if (!busy) onClose();
  };
  // The version found stays choosable even when the list lacks it.
  const gameOptions = game !== '' && !games.includes(game) ? [game, ...games] : games;

  return (
    <Dialog title={t.title} onClose={close} wide>
      {stage.step === 'choose' && (
        <>
          <p>{t.intro}</p>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button
              variant="primary"
              onClick={() => {
                pick(true);
              }}
            >
              {t.pickFolder}
            </Button>
            <Button
              onClick={() => {
                pick(false);
              }}
            >
              {t.pickZip}
            </Button>
            <Button onClick={onClose}>{t.cancel}</Button>
          </div>
        </>
      )}

      {stage.step === 'reading' && <p role="status">{t.reading}</p>}

      {(stage.step === 'review' || stage.step === 'creating') && (
        <>
          <p>
            <strong>{t.formats[stage.found.format]}</strong>
            <br />
            <span className={styles.muted}>
              {t.summary({
                files: stage.found.files,
                size: formatBytes(stage.found.bytes),
                mods: stage.found.mods,
              })}
            </span>
          </p>
          {stage.found.game === null ? (
            <Notice tone="warning">{t.noGame}</Notice>
          ) : (
            stage.found.guessed && <Notice tone="info">{t.guessed}</Notice>
          )}
          {stage.found.leftOut > 0 && (
            <p className={styles.muted}>{t.leftOut({ count: stage.found.leftOut })}</p>
          )}
          <Field label={t.name}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                value={name}
                maxLength={64}
                onChange={(event) => {
                  setName(event.target.value);
                }}
              />
            )}
          </Field>
          <Field label={t.game}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={game}
                onChange={(event) => {
                  setGame(event.target.value);
                }}
              >
                {game === '' && <option value="">—</option>}
                {gameOptions.map((g) => (
                  <option key={g} value={g}>
                    {g}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t.loader}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={loader}
                onChange={(event) => {
                  setLoader(event.target.value as LoaderKind | 'none');
                }}
              >
                <option value="none">{t.noLoader}</option>
                {MOD_LOADER_KINDS.map((kind) => (
                  <option key={kind} value={kind}>
                    {strings.loaders.names[kind]}
                  </option>
                ))}
              </select>
            )}
          </Field>
          {stage.step === 'creating' && <p role="status">{t.creating}</p>}
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button
              variant="primary"
              disabled={stage.step === 'creating' || name.trim() === '' || game === ''}
              onClick={() => {
                create(stage.found);
              }}
            >
              {t.create}
            </Button>
            <Button
              disabled={stage.step === 'creating'}
              onClick={() => {
                setError(null);
                setStage({ step: 'choose' });
              }}
            >
              {t.another}
            </Button>
          </div>
        </>
      )}

      {stage.step === 'copying' && (
        <>
          <div role="status" className={styles.progress}>
            <p>
              {progress !== null && progress.totalFiles > 0
                ? t.copying({ files: progress.files, total: progress.totalFiles })
                : t.creating}
            </p>
            {progress !== null && progress.totalBytes > 0 && (
              <progress max={progress.totalBytes} value={progress.bytes} />
            )}
          </div>
          <div className={forms.actions}>
            <Button
              onClick={() => {
                void ipc.modsCancel(stage.instance.id).catch(() => undefined);
              }}
            >
              {t.stop}
            </Button>
          </div>
        </>
      )}

      {stage.step === 'stopped' && (
        <>
          <Notice tone="warning">{t.stopped}</Notice>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button
              variant="primary"
              onClick={() => {
                copy(stage.instance);
              }}
            >
              {t.retry}
            </Button>
            <Button onClick={onClose}>{t.close}</Button>
          </div>
        </>
      )}

      {stage.step === 'done' && (
        <>
          <Notice tone="info">
            {t.done({
              name: stage.instance.displayName,
              count: stage.report.files + stage.report.kept,
            })}
          </Notice>
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
