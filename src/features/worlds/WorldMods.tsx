import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Tag } from '../../components/Tag';
import { useFocusBack } from '../../components/useFocusBack';
import { Field } from '../../components/Field';
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
  WorldsWorldMod,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { isolate } from '../mods/helpers';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

/** The mods named before "and N more"; all of them on request, and all at once when
 * only one would be left for the toggle. */
const SHOWN = 4;

/** The world's mods as the core read them, without entries that have no name. */
function namedMods(list: unknown): WorldsWorldMod[] {
  if (!Array.isArray(list)) return [];
  return list.filter(
    (mod: unknown): mod is WorldsWorldMod =>
      typeof mod === 'object' &&
      mod !== null &&
      typeof (mod as { name?: unknown }).name === 'string' &&
      (mod as { name: string }).name !== '',
  );
}

/** A mod's version beside its name, unless unknown or already in the name (a jar's
 * name such as "sodium-fabric-0.6.6"). */
function versionOf(mod: WorldsWorldMod): string | null {
  const version = mod.version;
  if (typeof version !== 'string' || version === '') return null;
  return mod.name.toLowerCase().includes(version.toLowerCase()) ? null : version;
}

/** "Sodium 0.6.6": the name and the version isolated each, as a title's own direction
 * must not carry the version along; a mod only the server runs says so. */
function ModTag({ mod }: { mod: WorldsWorldMod }) {
  const version = versionOf(mod);
  const server = mod.side === 'server';
  return (
    <Tag tone={server ? 'outline' : 'neutral'}>
      {/* One line of text: a pill lays out its children apart, spaces dropped. */}
      <span>
        <bdi>{mod.name}</bdi>
        {version !== null && (
          <>
            {' '}
            <bdi>{version}</bdi>
          </>
        )}
        {server && ` · ${strings.worlds.update.sides.server}`}
      </span>
    </Tag>
  );
}

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
  // Every installation: the world's own source is named even when it no longer fits.
  const [sources, setSources] = useState<WorldsModSource[] | null>(null);
  const [open, setOpen] = useState(false);
  const toolsId = useId();
  const toggle = useRef<HTMLButtonElement>(null);
  const [chosen, setChosen] = useState('');
  const [preview, setPreview] = useState<WorldsUpdatePreview | null>(null);
  const [choices, setChoices] = useState<Record<string, WorldsKeep>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [mods, setMods] = useState<WorldsWorldMod[] | null>(null);
  const [modsError, setModsError] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const modsId = useId();

  // Read again once a change of the world has finished: an update or a first start's
  // rollback (both get this room ready), a restore.
  const generation = world.generation;
  const changing = state.maintenance || state.preparing?.room === room;
  useEffect(() => {
    if (changing) return undefined;
    let live = true;
    ipc
      .worldsWorldMods(world.id)
      .then((list) => {
        if (!live) return;
        setMods(namedMods(list));
        setModsError(null);
      })
      .catch((e: unknown) => {
        if (live) setModsError(worldsErrorText(e));
      });
    return () => {
      live = false;
    };
  }, [world.id, generation, changing]);

  const folds = mods !== null && mods.length > SHOWN + 1;

  const kind = world.loader?.kind;
  useEffect(() => {
    ipc
      .worldsModSources()
      .then((all) => {
        setSources(Array.isArray(all) ? all : []);
      })
      .catch(() => {
        setSources([]);
      });
  }, []);

  const list = (sources ?? []).filter(
    (s) => s.minecraft === world.minecraft && s.loader.kind === kind,
  );
  const from = sources?.find((s) => s.instanceId === world.source)?.name ?? null;
  const picked = list.some((s) => s.instanceId === chosen)
    ? chosen
    : (list.find((s) => s.instanceId === world.source)?.instanceId ?? list[0]?.instanceId ?? '');
  const hosted = state.hosting?.world === world.id && state.hosting.clean === null;
  // The update's own press sets `maintenance`: that wait keeps the focus where it is.
  const waiting = state.preparing !== null || state.guest !== null;
  const working = busy || state.maintenance;
  const answered = preview !== null && preview.conflicts.every((c) => c.path in choices);
  // Update the world goes with the preview it answered: the focus goes to the toggle.
  useFocusBack(preview === null, toggle);

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
    if (working) return;
    setPreview(null);
    setChoices({});
    run(async () => {
      setPreview(await ipc.worldsUpdatePreview(world.id, picked));
      return null;
    });
  };

  const apply = (shown: WorldsUpdatePreview) => {
    if (working) return;
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
    <Panel
      title={t.title}
      level={3}
      count={world.mods}
      aside={
        <Button
          ref={toggle}
          size="small"
          aria-expanded={open}
          aria-controls={open ? toolsId : undefined}
          onClick={() => {
            setOpen((now) => !now);
          }}
        >
          {t.open}
        </Button>
      }
    >
      {world.generation !== null && (
        <p className={styles.panelHint}>
          {t.current({ generation: world.generation, count: world.mods })}
        </p>
      )}
      <p className={styles.panelHint}>
        {from !== null && `${strings.worlds.modsFrom({ name: isolate(from) })} `}
        {strings.worlds.modsGuests}
      </p>
      {mods !== null && mods.length > 0 && (
        <div className={styles.modChips}>
          <ul id={modsId} className={styles.chipList}>
            {(all || !folds ? mods : mods.slice(0, SHOWN)).map((mod, index) => (
              <li key={`${String(index)}:${mod.name}`}>
                <ModTag mod={mod} />
              </li>
            ))}
          </ul>
          {folds && (
            <button
              type="button"
              className={styles.link}
              aria-expanded={all}
              aria-controls={modsId}
              onClick={() => {
                setAll((now) => !now);
              }}
            >
              {all ? t.fewer : t.more({ count: mods.length - SHOWN })}
            </button>
          )}
        </div>
      )}
      {modsError !== null && <p className={styles.panelHint}>{modsError}</p>}
      {world.updatePending && <Notice tone="info">{t.pending}</Notice>}
      {open && (
        <div id={toolsId} className={styles.tools}>
          <p className={styles.panelHint}>{t.hint}</p>
          {sources !== null && list.length === 0 ? (
            <p className={styles.panelHint}>{t.noSources({ version: world.minecraft })}</p>
          ) : (
            <div className={styles.toolsRow}>
              <Field label={t.source}>
                {(field) => (
                  <select
                    {...field}
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
                )}
              </Field>
              <Button
                disabled={waiting || picked === ''}
                aria-disabled={working || undefined}
                onClick={show}
              >
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
                {preview.loaderChanged && (
                  <p>{t.loaderChanged({ version: preview.loader.version })}</p>
                )}
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
                    disabled={waiting || !answered}
                    aria-disabled={working || undefined}
                    onClick={() => {
                      apply(preview);
                    }}
                  >
                    {t.apply}
                  </Button>
                </div>
              </div>
            ))}
        </div>
      )}
      <p className={styles.live} role="status">
        {busy && preview === null ? strings.backups.working : (note ?? '')}
      </p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </Panel>
  );
}
