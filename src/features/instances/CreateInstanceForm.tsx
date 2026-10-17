import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  CatalogView,
  PlanSummary,
  SupportedVersion,
  VersionAvailability,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';

interface CreateInstanceFormProps {
  versions: SupportedVersion[];
  onDone: () => void;
}

const t = strings.createForm;

const availabilityTone: Record<VersionAvailability, StatusTone> = {
  available: 'success',
  descriptor_changed: 'warning',
  not_in_catalog: 'danger',
  catalog_unavailable: 'warning',
};

export function CreateInstanceForm({ versions, onDone }: CreateInstanceFormProps) {
  const [name, setName] = useState('');
  const [versionId, setVersionId] = useState(versions[0]?.id ?? '');
  const [catalog, setCatalog] = useState<CatalogView | null>(null);
  const [checking, setChecking] = useState(true);
  const [plan, setPlan] = useState<PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [estimating, setEstimating] = useState(false);
  const create = useCommand((requestId: string, displayName: string, version: string) =>
    ipc.createInstance({ displayName, versionId: version }, requestId),
  );

  // The backend reports catalog outages inside the view; a rejection only means
  // the core did not answer, which the availability badge shows as unreachable.
  const fetchCatalog = useCallback(
    (refresh: boolean) =>
      ipc
        .listSupportedVersions(refresh)
        .then(setCatalog)
        .catch(() => {
          setCatalog(null);
        })
        .finally(() => {
          setChecking(false);
        }),
    [],
  );

  useEffect(() => {
    void fetchCatalog(false);
  }, [fetchCatalog]);

  const loadCatalog = (refresh: boolean) => {
    setChecking(true);
    void fetchCatalog(refresh);
  };

  const selected = catalog?.versions.find((version) => version.id === versionId);
  const availability = selected?.availability ?? 'catalog_unavailable';

  const estimate = async () => {
    setEstimating(true);
    setPlanError(null);
    try {
      setPlan(await ipc.previewInstallPlan(versionId));
    } catch (error) {
      setPlan(null);
      setPlanError(describeIpcError(error));
    } finally {
      setEstimating(false);
    }
  };

  const submit = async () => {
    const created = await create.run(name, versionId);
    if (created) onDone();
  };

  return (
    <Panel title={t.title}>
      <form
        className={forms.form}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
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
              autoComplete="off"
              onChange={(event) => {
                setName(event.target.value);
              }}
            />
          )}
        </Field>
        <Field label={t.version} hint={t.isolation}>
          {(props) => (
            <select
              {...props}
              className={forms.select}
              value={versionId}
              onChange={(event) => {
                setVersionId(event.target.value);
                setPlan(null);
                setPlanError(null);
              }}
            >
              {versions.map((version) => (
                <option key={version.id} value={version.id}>
                  {version.id}
                  {version.status === 'pending_verification' ? ` (${t.pending})` : ''}
                </option>
              ))}
            </select>
          )}
        </Field>
        <div className={forms.actions}>
          <span role="status">
            {checking ? (
              <span className={styles.muted}>{t.availability.checking}</span>
            ) : (
              <StatusBadge
                tone={availabilityTone[availability]}
                label={t.availability[availability]}
              />
            )}
          </span>
          <Button
            disabled={checking}
            onClick={() => {
              loadCatalog(true);
            }}
          >
            {t.refresh}
          </Button>
          <Button
            disabled={estimating || availability !== 'available'}
            onClick={() => void estimate()}
          >
            {estimating ? t.estimating : t.estimate}
          </Button>
        </div>
        {catalog?.stale === true && <Notice tone="warning">{t.stale}</Notice>}
        {planError !== null && <Notice tone="danger">{planError}</Notice>}
        {plan !== null && <PlanFacts plan={plan} />}
        {create.error !== null && <Notice tone="danger">{create.error}</Notice>}
        <div className={forms.actions}>
          <Button variant="primary" type="submit" disabled={create.pending || name.trim() === ''}>
            {t.submit}
          </Button>
          <Button onClick={onDone}>{t.cancel}</Button>
        </div>
      </form>
    </Panel>
  );
}

function PlanFacts({ plan }: { plan: PlanSummary }) {
  return (
    <section aria-label={t.planTitle(plan.versionId)}>
      <dl className={styles.facts}>
        <dt>{t.planJava}</dt>
        <dd>{plan.javaMajor}</dd>
        <dt>{t.planLibraries}</dt>
        <dd>{plan.libraries}</dd>
        <dt>{t.planAssets}</dt>
        <dd>{plan.assetObjects}</dd>
        <dt>{t.planGameFiles}</dt>
        <dd>
          {plan.gameArtifacts} ({formatBytes(plan.gameBytes)})
        </dd>
        <dt>{t.planMissing}</dt>
        <dd>{formatBytes(plan.missingGameBytes)}</dd>
        <dt>{t.planRuntime}</dt>
        <dd>
          {plan.runtimeBytes === null ? t.planRuntimeUnknown : formatBytes(plan.runtimeBytes)}
        </dd>
      </dl>
      <p className={styles.muted}>{t.planNote}</p>
    </section>
  );
}
