import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { WorldsImportable, WorldsRoom } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { gameLabel } from '../instances/loaderLabel';
import { Dialog } from '../../components/Dialog';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  onClose: () => void;
  onImported: (room: WorldsRoom) => void;
  /** The list the screen read already: every installation's saves are not read twice. */
  known?: WorldsImportable[] | undefined;
  /** The world chosen on the screen, when it is in that list. */
  initial?: { instanceId: string; save: string } | undefined;
}

/** The installation and world to start with: the one chosen, else the first. */
function start(list: WorldsImportable[], initial: Props['initial']) {
  const chosen =
    initial === undefined
      ? undefined
      : list.find((s) => s.instanceId === initial.instanceId && s.saves.includes(initial.save));
  if (chosen !== undefined && initial !== undefined) {
    return { instance: chosen.instanceId, save: initial.save };
  }
  const first = list[0];
  return { instance: first?.instanceId ?? '', save: first?.saves[0] ?? '' };
}

/**
 * A single-player world brought into worlds with friends (W9): it is copied,
 * the original stays in its installation. The EULA is accepted here.
 */
export function ImportWorld({ onClose, onImported, known, initial }: Props) {
  const t = strings.worlds.import;
  const form = strings.worlds.form;
  const first = known === undefined ? null : start(known, initial);
  const [sources, setSources] = useState<WorldsImportable[] | null>(known ?? null);
  const [instance, setInstance] = useState(first?.instance ?? '');
  const [save, setSave] = useState(first?.save ?? '');
  const [name, setName] = useState(first?.save ?? '');
  const [description, setDescription] = useState('');
  const [eula, setEula] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (known !== undefined) return;
    ipc
      .worldsImportable()
      .then((list) => {
        setSources(list);
        const chosen = start(list, undefined);
        setInstance(chosen.instance);
        setSave(chosen.save);
        setName(chosen.save);
      })
      .catch((e: unknown) => {
        setSources([]);
        setError(worldsErrorText(e));
      });
    // The list given at the start stays the dialog's list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const source = sources?.find((s) => s.instanceId === instance) ?? null;

  const choose = (nextInstance: string, nextSave: string) => {
    // The name follows the chosen world until the player changes it.
    if (name === save) setName(nextSave);
    setInstance(nextInstance);
    setSave(nextSave);
  };

  const ready = eula && source !== null && save !== '' && name.trim() !== '';

  const submit = () => {
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    void (async () => {
      try {
        const room = await ipc.worldsImport({
          instanceId: instance,
          save,
          name: name.trim(),
          description: description.trim(),
        });
        if (room.world !== null) await ipc.worldsAcceptEula(room.world);
        onImported(room);
      } catch (e: unknown) {
        setError(worldsErrorText(e));
        setBusy(false);
      }
    })();
  };

  return (
    <Dialog title={t.title} onClose={onClose} wide>
      <p className={styles.muted}>{t.hint}</p>
      {sources === null ? (
        <p className={styles.muted}>{strings.worlds.loading}</p>
      ) : sources.length === 0 ? (
        error === null && <Notice tone="info">{t.none}</Notice>
      ) : (
        <form
          className={styles.form}
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <div className={forms.row}>
            <Field label={t.installation}>
              {(props) => (
                <select
                  {...props}
                  className={forms.select}
                  value={instance}
                  onChange={(e) => {
                    const next = sources.find((s) => s.instanceId === e.target.value);
                    choose(e.target.value, next?.saves[0] ?? '');
                  }}
                >
                  {sources.map((s) => (
                    <option key={s.instanceId} value={s.instanceId}>
                      {form.modSource({
                        name: s.instanceName,
                        game: gameLabel({ versionId: s.minecraft, loader: s.loader }),
                      })}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label={t.world}>
              {(props) => (
                <select
                  {...props}
                  className={forms.select}
                  value={save}
                  onChange={(e) => {
                    choose(instance, e.target.value);
                  }}
                >
                  {source?.saves.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              )}
            </Field>
          </div>
          <Field label={form.name}>
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
          <Field label={form.description}>
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
          <label className={styles.check}>
            <input
              type="checkbox"
              checked={eula}
              onChange={(e) => {
                setEula(e.target.checked);
              }}
            />
            {form.eula}
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
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button onClick={onClose}>{strings.worlds.cancel}</Button>
            {/* Pressed, it keeps the focus while the world is copied. */}
            <Button
              type="submit"
              variant="primary"
              disabled={!ready}
              aria-disabled={busy || undefined}
            >
              {busy ? t.importing : t.submit}
            </Button>
          </div>
        </form>
      )}
      {sources !== null && sources.length === 0 && (
        <>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={forms.actions}>
            <Button onClick={onClose}>{strings.worlds.close}</Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
