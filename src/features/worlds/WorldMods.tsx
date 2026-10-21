import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  LocalWorld,
  WorldsKeep,
  WorldsModChange,
  WorldsModSource,
  WorldsSettingConflict,
  WorldsShownSetting,
  WorldsState,
  WorldsUpdatePreview,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  room: string;
  world: LocalWorld;
  state: WorldsState;
}

function Changes({ title, list }: { title: string; list: WorldsModChange[] }) {
  const t = strings.worlds.update;
  if (list.length === 0) return null;
  return (
    <div>
      <p className={styles.statusTitle}>{title}</p>
      <ul className={styles.changes}>
        {list.map((change) => (
          <li key={change.name}>
            {change.from !== undefined && change.from !== change.name
              ? t.was({ name: change.name, from: change.from })
              : change.name}{' '}
            <span className={styles.muted}>· {t.sides[change.side]}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Side({ title, shown }: { title: string; shown: WorldsShownSetting | null }) {
  const t = strings.worlds.update;
  return (
    <div className={styles.side}>
      <p className={styles.muted}>{title}</p>
      {shown === null ? (
        <p className={styles.muted}>{t.noBase}</p>
      ) : shown.text === null ? (
        <p className={styles.muted}>{t.notText({ size: formatBytes(shown.bytes) })}</p>
      ) : (
        <>
          <pre className={styles.text}>{shown.text}</pre>
          {shown.truncated && <p className={styles.muted}>{t.cut}</p>}
        </>
      )}
    </div>
  );
}

function ConflictView({
  conflict,
  keep,
  onKeep,
}: {
  conflict: WorldsSettingConflict;
  keep: WorldsKeep | undefined;
  onKeep: (keep: WorldsKeep) => void;
}) {
  const t = strings.worlds.update;
  return (
    <li className={styles.room}>
      <span className={styles.nick}>{conflict.path}</span>
      <div className={styles.threeWay}>
        <Side title={t.base} shown={conflict.base} />
        <Side title={t.mine} shown={conflict.mine} />
        <Side title={t.theirs} shown={conflict.theirs} />
      </div>
      <div className={styles.actions} role="radiogroup" aria-label={conflict.path}>
        {(['mine', 'theirs'] as const).map((option) => (
          <label key={option} className={styles.check}>
            <input
              type="radio"
              name={conflict.path}
              checked={keep === option}
              onChange={() => {
                onKeep(option);
              }}
            />
            {option === 'mine' ? t.keepMine : t.takeTheirs}
          </label>
        ))}
      </div>
    </li>
  );
}

/**
 * The mods of a world with mods (W12): updated from an installation of the
 * player's, after they saw what changes and answered for the server settings
 * they changed. The world is backed up and the mods tried on a copy first; a
 * hosted world closes for it and opens again.
 */
export function WorldMods({ room, world, state }: Props) {
  const t = strings.worlds.update;
  const [sources, setSources] = useState<WorldsModSource[] | null>(null);
  const [chosen, setChosen] = useState('');
  const [preview, setPreview] = useState<WorldsUpdatePreview | null>(null);
  const [choices, setChoices] = useState<Record<string, WorldsKeep>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const kind = world.loader?.kind;
  useEffect(() => {
    ipc
      .worldsModSources()
      .then((all) => {
        setSources(all.filter((s) => s.minecraft === world.minecraft && s.loader.kind === kind));
      })
      .catch(() => {
        setSources([]);
      });
  }, [world.minecraft, kind]);

  const list = sources ?? [];
  const picked = list.some((s) => s.instanceId === chosen)
    ? chosen
    : (list.find((s) => s.instanceId === world.source)?.instanceId ?? list[0]?.instanceId ?? '');
  const hosted = state.hosting?.world === world.id && state.hosting.clean === null;
  const waiting = state.maintenance || state.preparing !== null || state.guest !== null;
  const answered = preview !== null && preview.conflicts.every((c) => c.path in choices);

  const run = (action: () => Promise<string | null>) => {
    setBusy(true);
    setError(null);
    setNote(null);
    action()
      .then(setNote)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const show = () => {
    setPreview(null);
    setChoices({});
    run(async () => {
      setPreview(await ipc.worldsUpdatePreview(world.id, picked));
      return null;
    });
  };

  const apply = (shown: WorldsUpdatePreview) => {
    const reopen = hosted;
    run(async () => {
      if (reopen) await ipc.worldsStopHosting();
      await ipc.worldsUpdate({
        room,
        world: world.id,
        instanceId: picked,
        fingerprint: shown.fingerprint,
        choices,
      });
      setPreview(null);
      if (reopen) await ipc.worldsHost(room);
      return t.done({ generation: shown.next });
    });
  };

  return (
    <section className={styles.section} aria-label={t.title}>
      <h4 className={styles.sectionTitle}>{t.title}</h4>
      {world.generation !== null && (
        <p>{t.current({ generation: world.generation, count: world.mods })}</p>
      )}
      {world.updatePending && <Notice tone="info">{t.pending}</Notice>}
      <p className={styles.muted}>{t.hint}</p>
      {sources !== null && list.length === 0 ? (
        <p className={styles.muted}>{t.noSources({ version: world.minecraft })}</p>
      ) : (
        <div className={forms.row}>
          <label>
            {t.source}{' '}
            <select
              className={forms.select}
              value={picked}
              onChange={(e) => {
                setChosen(e.target.value);
                setPreview(null);
              }}
            >
              {list.map((source) => (
                <option key={source.instanceId} value={source.instanceId}>
                  {source.name}
                </option>
              ))}
            </select>
          </label>
          <Button disabled={busy || waiting || picked === ''} onClick={show}>
            {t.show}
          </Button>
        </div>
      )}
      {preview !== null &&
        (preview.same ? (
          <p className={styles.muted}>{t.same}</p>
        ) : (
          <div className={styles.status}>
            <p>{t.next({ generation: preview.next })}</p>
            {preview.loaderChanged && <p>{t.loaderChanged({ version: preview.loader.version })}</p>}
            <Changes title={t.added} list={preview.added} />
            <Changes title={t.removed} list={preview.removed} />
            <Changes title={t.updated} list={preview.updated} />
            {preview.settings > 0 && (
              <p className={styles.muted}>{t.settings({ count: preview.settings })}</p>
            )}
            {preview.conflicts.length > 0 && (
              <>
                <p className={styles.statusTitle}>{t.conflictsTitle}</p>
                <p className={styles.muted}>{t.conflictsHint}</p>
                <ul className={styles.members}>
                  {preview.conflicts.map((conflict) => (
                    <ConflictView
                      key={conflict.path}
                      conflict={conflict}
                      keep={choices[conflict.path]}
                      onKeep={(keep) => {
                        setChoices((current) => ({ ...current, [conflict.path]: keep }));
                      }}
                    />
                  ))}
                </ul>
              </>
            )}
            {hosted && <p className={styles.muted}>{t.whileHosted}</p>}
            <div className={styles.actions}>
              <Button
                variant="primary"
                disabled={busy || waiting || !answered}
                onClick={() => {
                  apply(preview);
                }}
              >
                {t.apply}
              </Button>
            </div>
          </div>
        ))}
      {busy && preview === null && (
        <p className={styles.muted} role="status">
          {strings.backups.working}
        </p>
      )}
      {note !== null && (
        <p className={styles.muted} role="status">
          {note}
        </p>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </section>
  );
}
