import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
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
}: {
  instance: InstanceSummary;
  loader: LoaderChoice;
  queued: boolean;
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
  const busy = instance.busy || change.pending;

  return (
    <form
      className={forms.form}
      aria-label={t.title}
      onSubmit={(event) => {
        event.preventDefault();
        setDone(null);
        void change.run(picked).then((saved) => {
          if (saved) setDone(saved.loader?.version ?? picked);
        });
      }}
    >
      <h3 className={styles.subheading}>{t.title}</h3>
      <p className={styles.hint}>{t.now({ loader: name, version: label(loader.version) })}</p>
      <Field label={tl.version({ loader: name })} hint={t.hint}>
        {(props) => (
          <select
            {...props}
            className={forms.select}
            value={picked}
            disabled={listed === null || busy}
            onChange={(event) => {
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
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {changed && (
        <Notice tone="warning">
          {t.warning({ loader: name, from: label(loader.version), to: label(picked) })}
        </Notice>
      )}
      {change.error !== null && <Notice tone="danger">{change.error}</Notice>}
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={!changed || busy}>
          {t.apply}
        </Button>
        {done !== null && (
          <span role="status">{t.applied({ loader: name, version: label(done) })}</span>
        )}
      </div>
    </form>
  );
}
