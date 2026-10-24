import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, LoaderChoice, LoaderVersion } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { loaderVersionLabel } from '../instances/loaderLabel';
import styles from './SettingsView.module.css';

const t = strings.settings.instanceLoader;
const tl = strings.loaders;

function optionLabel(version: LoaderVersion): string {
  return version.recommended
    ? tl.recommended({ label: version.label })
    : version.stable
      ? version.label
      : tl.unstable({ label: version.label });
}

/**
 * Another version of an installation's mod loader (D-054): saved, then
 * installed at once. The game files, mods and worlds stay. Keyed by instance id
 * by the parent; shown only for installations with a loader. An install already
 * waiting in the queue picks the new version up when it starts.
 */
export function InstanceLoaderForm({
  instance,
  loader,
  queued,
  level = 3,
}: {
  instance: InstanceSummary;
  loader: LoaderChoice;
  queued: boolean;
  /** The panel's heading level: 4 under an installation's own settings title. */
  level?: 3 | 4;
}) {
  const [versions, setVersions] = useState<LoaderVersion[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [picked, setPicked] = useState(loader.version);
  const [done, setDone] = useState<string | null>(null);
  const change = useCommand(async (requestId: string, version: string) => {
    const saved = await ipc.setInstanceLoader({ instanceId: instance.id, version }, requestId);
    if (!queued) await ipc.installInstance(instance.id);
    return saved;
  });
  const name = tl.names[loader.kind];

  useEffect(() => {
    let current = true;
    ipc
      .listLoaderVersions(loader.kind, instance.versionId)
      .then((found) => {
        if (current) setVersions(found);
      })
      .catch((error: unknown) => {
        if (current) setLoadError(describeIpcError(error));
      });
    return () => {
      current = false;
    };
  }, [loader.kind, instance.versionId]);

  const label = (version: string) =>
    loaderVersionLabel({ kind: loader.kind, version }, instance.versionId);
  // The installed version stays listed even if its project withdrew it.
  const listed =
    versions === null || versions.some((v) => v.id === loader.version)
      ? versions
      : [
          { id: loader.version, label: label(loader.version), stable: true, recommended: false },
          ...versions,
        ];
  const changed = picked !== loader.version;
  // Changing and the install after it keep the list and the button as they are (a
  // disabled control drops the focus). The button closes once the installation has the
  // new version, before or after the answer: the focus goes to what was done, or to the
  // list until it is said.
  const busy = instance.busy;
  const held = change.pending || busy;
  const appliedLine = useRef<HTMLSpanElement>(null);
  const select = useRef<HTMLSelectElement>(null);
  const hintId = useId();
  useFocusBack(`${String(done !== null)}:${String(changed)}`, appliedLine, select);

  return (
    <Panel title={t.title} level={level}>
      <form
        className={forms.form}
        onSubmit={(event) => {
          event.preventDefault();
          if (held || !changed) return;
          setDone(null);
          void change.run(picked).then((saved) => {
            if (saved) setDone(saved.loader?.version ?? picked);
          });
        }}
      >
        <p>{t.now({ loader: name, version: label(loader.version) })}</p>
        {/* The version and the button that installs it, side by side while there is room. */}
        <div className={styles.inlineRow}>
          <Field label={tl.version({ loader: name })}>
            {(props) => (
              <select
                {...props}
                aria-describedby={hintId}
                ref={select}
                className={[forms.select, styles.long].join(' ')}
                value={picked}
                disabled={listed === null}
                aria-disabled={held || undefined}
                onChange={(event) => {
                  if (held) return;
                  setPicked(event.target.value);
                  setDone(null);
                }}
              >
                {listed === null && <option value={loader.version}>{tl.loading}</option>}
                {listed?.map((version) => (
                  <option key={version.id} value={version.id}>
                    {optionLabel(version)}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Button type="submit" disabled={!changed} aria-disabled={held || undefined}>
            {t.apply}
          </Button>
        </div>
        <p id={hintId} className={styles.hint}>
          {t.hint}
        </p>
        {done !== null && (
          <span ref={appliedLine} role="status" className={styles.saved} tabIndex={-1}>
            {t.applied({ loader: name, version: label(done) })}
          </span>
        )}
        {changed && (
          <Notice tone="warning">
            {t.warning({ loader: name, from: label(loader.version), to: label(picked) })}
          </Notice>
        )}
        {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
        {change.error !== null && <Notice tone="danger">{change.error}</Notice>}
      </form>
    </Panel>
  );
}
