import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  WorldsDifficulty,
  WorldsGameMode,
  WorldsModSource,
  WorldsPreset,
  WorldsRoom,
  WorldsSettings,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import { Dialog } from '../mods/Dialog';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

const MODES: WorldsGameMode[] = ['survival', 'creative', 'adventure'];
const DIFFICULTIES: WorldsDifficulty[] = ['peaceful', 'easy', 'normal', 'hard'];
const PRESETS: WorldsPreset[] = ['normal', 'flat', 'large_biomes', 'amplified'];
const MAX_PLAYERS = 20;

const DEFAULTS: WorldsSettings = {
  seed: '',
  preset: 'normal',
  difficulty: 'normal',
  gameMode: 'survival',
  hardcore: false,
  pvp: true,
  maxPlayers: 8,
};

interface Props {
  onClose: () => void;
  onCreated: (room: WorldsRoom) => void;
}

/** A new world on this computer and its room; the EULA is accepted here. */
export function CreateWorld({ onClose, onCreated }: Props) {
  const t = strings.worlds.form;
  const [versions, setVersions] = useState<string[]>([]);
  const [snapshots, setSnapshots] = useState(false);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [minecraft, setMinecraft] = useState('');
  const [sources, setSources] = useState<WorldsModSource[]>([]);
  /** The installation whose mods the world takes; empty for none. */
  const [from, setFrom] = useState('');
  const [settings, setSettings] = useState<WorldsSettings>(DEFAULTS);
  const [eula, setEula] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipc
      .worldsVersions(snapshots)
      .then((list) => {
        setVersions(list);
        setMinecraft((current) => (list.includes(current) ? current : (list[0] ?? '')));
      })
      .catch(() => undefined);
  }, [snapshots]);

  useEffect(() => {
    void ipc
      .worldsModSources()
      .then(setSources)
      .catch(() => undefined);
  }, []);

  const source = sources.find((s) => s.instanceId === from) ?? null;
  // A world with mods plays its installation's version.
  const version = source?.minecraft ?? minecraft;

  const set = <K extends keyof WorldsSettings>(key: K, value: WorldsSettings[K]) => {
    setSettings((current) => ({ ...current, [key]: value }));
  };

  const submit = () => {
    if (!eula || name.trim() === '' || version === '') return;
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        const room = await ipc.worldsCreate({
          name: name.trim(),
          description: description.trim(),
          minecraft: version,
          settings,
          instanceId: source?.instanceId ?? null,
        });
        if (room.world !== null) await ipc.worldsAcceptEula(room.world);
        onCreated(room);
      } catch (e: unknown) {
        setError(worldsErrorText(e));
        setBusy(false);
      }
    })();
  };

  return (
    <Dialog title={strings.worlds.create} onClose={onClose} wide>
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label={t.name}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              value={name}
              maxLength={64}
              required
              onChange={(e) => {
                setName(e.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.description}>
          {(props) => (
            <textarea
              {...props}
              className={forms.input}
              value={description}
              maxLength={500}
              rows={2}
              onChange={(e) => {
                setDescription(e.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.mods}>
          {(props) => (
            <select
              {...props}
              className={forms.select}
              value={from}
              onChange={(e) => {
                setFrom(e.target.value);
              }}
            >
              <option value="">{t.noMods}</option>
              {sources.map((s) => (
                <option key={s.instanceId} value={s.instanceId}>
                  {t.modSource({
                    name: s.name,
                    game: gameLabel({ versionId: s.minecraft, loader: s.loader }),
                  })}
                </option>
              ))}
            </select>
          )}
        </Field>
        <p className={styles.muted}>{source === null ? t.modsHow : t.modsNote}</p>
        <div className={forms.row}>
          <Field label={t.version}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={version}
                disabled={source !== null}
                onChange={(e) => {
                  setMinecraft(e.target.value);
                }}
              >
                {(source === null ? versions : [source.minecraft]).map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t.gameMode}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={settings.gameMode}
                onChange={(e) => {
                  set('gameMode', e.target.value as WorldsGameMode);
                }}
              >
                {MODES.map((m) => (
                  <option key={m} value={m}>
                    {t.modes[m]}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t.difficulty}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={settings.difficulty}
                onChange={(e) => {
                  set('difficulty', e.target.value as WorldsDifficulty);
                }}
              >
                {DIFFICULTIES.map((d) => (
                  <option key={d} value={d}>
                    {t.difficulties[d]}
                  </option>
                ))}
              </select>
            )}
          </Field>
        </div>
        {source === null && (
          <label className={styles.check}>
            <input
              type="checkbox"
              checked={snapshots}
              onChange={(e) => {
                setSnapshots(e.target.checked);
              }}
            />
            {t.snapshots}
          </label>
        )}
        {source === null && snapshots && <Notice tone="warning">{t.snapshotsNote}</Notice>}
        <div className={forms.row}>
          <Field label={t.preset}>
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={settings.preset}
                onChange={(e) => {
                  set('preset', e.target.value as WorldsPreset);
                }}
              >
                {PRESETS.map((p) => (
                  <option key={p} value={p}>
                    {t.presets[p]}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t.seed} hint={t.seedHint}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                value={settings.seed}
                maxLength={64}
                onChange={(e) => {
                  set('seed', e.target.value);
                }}
              />
            )}
          </Field>
          <Field label={t.maxPlayers}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                type="number"
                min={1}
                max={MAX_PLAYERS}
                value={settings.maxPlayers}
                onChange={(e) => {
                  const value = Math.round(Number(e.target.value));
                  if (Number.isFinite(value)) {
                    set('maxPlayers', Math.min(MAX_PLAYERS, Math.max(1, value)));
                  }
                }}
              />
            )}
          </Field>
        </div>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={settings.hardcore}
            onChange={(e) => {
              set('hardcore', e.target.checked);
            }}
          />
          {t.hardcore}
        </label>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={settings.pvp}
            onChange={(e) => {
              set('pvp', e.target.checked);
            }}
          />
          {t.pvp}
        </label>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={eula}
            onChange={(e) => {
              setEula(e.target.checked);
            }}
          />
          {t.eula}
        </label>
        <div className={styles.actions}>
          <Button
            onClick={() => {
              void ipc.openExternal('minecraft_eula').catch(() => undefined);
            }}
          >
            {strings.worlds.readEula}
          </Button>
        </div>
        <p className={styles.muted}>{t.keptHint}</p>
        {error !== null && <Notice tone="danger">{error}</Notice>}
        <div className={forms.actions}>
          <Button onClick={onClose}>{strings.worlds.cancel}</Button>
          <Button
            type="submit"
            variant="primary"
            disabled={busy || !eula || name.trim() === '' || version === ''}
          >
            {busy ? t.creating : t.submit}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
