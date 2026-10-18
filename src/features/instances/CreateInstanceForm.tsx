import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  CatalogView,
  LoaderChoice,
  LoaderKind,
  LoaderVersion,
  NetworkStatus,
  PlanSummary,
  VersionAvailability,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';
import { VersionPicker } from './VersionPicker';
import { LOADER_KINDS } from './loaderLabel';

interface CreateInstanceFormProps {
  onDone: () => void;
  /** When the connection returns, the version list is fetched again (D-052). */
  network?: NetworkStatus;
}

const t = strings.createForm;
const tl = strings.loaders;

/** The default name: the version, and the loader when there is one. */
function defaultName(versionId: string, kind: LoaderKind | 'none'): string {
  return kind === 'none' ? versionId : `${versionId} ${tl.names[kind]}`;
}

const availabilityTone: Record<VersionAvailability, StatusTone> = {
  available: 'success',
  not_in_catalog: 'danger',
  catalog_unavailable: 'warning',
};

export function CreateInstanceForm({ onDone, network = 'online' }: CreateInstanceFormProps) {
  // The name follows the chosen version and loader until the user types one.
  const [typedName, setTypedName] = useState<string | null>(null);
  const [versionId, setVersionId] = useState('');
  const [catalog, setCatalog] = useState<CatalogView | null>(null);
  const [checking, setChecking] = useState(true);
  const [plan, setPlan] = useState<PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [estimating, setEstimating] = useState(false);
  // Mod loader (D-039): the game versions each loader supports, its versions
  // per game version (by `kind:game`), and the user's pick among them.
  const [loaderKind, setLoaderKind] = useState<LoaderKind | 'none'>('none');
  const [gamesByKind, setGamesByKind] = useState<Partial<Record<LoaderKind, ReadonlySet<string>>>>(
    {},
  );
  const [versionsByKey, setVersionsByKey] = useState<Record<string, LoaderVersion[]>>({});
  const [pickedByKey, setPickedByKey] = useState<Record<string, string>>({});
  const [loaderError, setLoaderError] = useState<{ key: string; message: string } | null>(null);
  const create = useCommand(
    (requestId: string, displayName: string, version: string, loader: LoaderChoice | null) =>
      ipc.createInstance({ displayName, versionId: version, loader }, requestId),
  );

  const loaderKey = loaderKind === 'none' ? '' : `${loaderKind}:${versionId}`;
  const loaderGames = loaderKind === 'none' ? null : (gamesByKind[loaderKind] ?? null);
  const loaderVersions = loaderKey === '' ? null : (versionsByKey[loaderKey] ?? null);
  const preferred =
    loaderVersions?.find((v) => v.recommended) ??
    loaderVersions?.find((v) => v.stable) ??
    loaderVersions?.[0];
  const loaderVersion = pickedByKey[loaderKey] ?? preferred?.id ?? '';
  const loader: LoaderChoice | null =
    loaderKind === 'none' || loaderVersion === ''
      ? null
      : { kind: loaderKind, version: loaderVersion };
  const name = typedName ?? (versionId === '' ? '' : defaultName(versionId, loaderKind));
  const shownError =
    loaderError !== null && (loaderError.key === loaderKey || loaderError.key === loaderKind)
      ? loaderError.message
      : null;

  // The backend reports catalog outages inside the view; a rejection only means
  // the core did not answer, which the availability badge shows as unreachable.
  const fetchCatalog = useCallback(
    (refresh: boolean) =>
      ipc
        .listSupportedVersions(refresh)
        .then((view) => {
          setCatalog(view);
          // Preselect the latest release.
          const initial =
            view.versions.find((v) => v.id === view.latestRelease) ?? view.versions[0];
          if (initial !== undefined) {
            setVersionId((current) => (current === '' ? initial.id : current));
          }
        })
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

  // Back online: check the version list again.
  const wasOffline = useRef(network === 'offline');
  useEffect(() => {
    if (network === 'online' && wasOffline.current) void fetchCatalog(true);
    wasOffline.current = network === 'offline';
  }, [network, fetchCatalog]);

  const loadCatalog = (refresh: boolean) => {
    setChecking(true);
    void fetchCatalog(refresh);
  };

  /** Moves the selection to the newest supported release when needed. */
  const keepSupported = useCallback(
    (allowed: ReadonlySet<string>) => {
      setVersionId((selectedId) => {
        if (allowed.has(selectedId) || catalog === null) return selectedId;
        const fallback =
          catalog.versions.find((v) => v.versionType === 'release' && allowed.has(v.id)) ??
          catalog.versions.find((v) => allowed.has(v.id));
        return fallback?.id ?? selectedId;
      });
    },
    [catalog],
  );

  // The game versions the chosen loader supports, fetched once per loader.
  useEffect(() => {
    if (loaderKind === 'none' || gamesByKind[loaderKind] !== undefined) return;
    let current = true;
    ipc
      .listLoaderGameVersions(loaderKind)
      .then((games) => {
        if (!current) return;
        const allowed = new Set(games);
        setGamesByKind((known) => ({ ...known, [loaderKind]: allowed }));
        keepSupported(allowed);
      })
      .catch((error: unknown) => {
        if (current) setLoaderError({ key: loaderKind, message: describeIpcError(error) });
      });
    return () => {
      current = false;
    };
  }, [loaderKind, gamesByKind, keepSupported]);

  // The loader's versions for the chosen game version, fetched once each.
  useEffect(() => {
    if (
      loaderKind === 'none' ||
      versionId === '' ||
      loaderGames?.has(versionId) !== true ||
      versionsByKey[loaderKey] !== undefined
    ) {
      return;
    }
    let current = true;
    ipc
      .listLoaderVersions(loaderKind, versionId)
      .then((versions) => {
        if (current) setVersionsByKey((known) => ({ ...known, [loaderKey]: versions }));
      })
      .catch((error: unknown) => {
        if (current) setLoaderError({ key: loaderKey, message: describeIpcError(error) });
      });
    return () => {
      current = false;
    };
  }, [loaderKind, versionId, loaderGames, loaderKey, versionsByKey]);

  const selected = catalog?.versions.find((version) => version.id === versionId);
  const availability = selected?.availability ?? 'catalog_unavailable';
  const loaderReady = loaderKind === 'none' || loader !== null;

  const estimate = async () => {
    setEstimating(true);
    setPlanError(null);
    try {
      setPlan(await ipc.previewInstallPlan(versionId, loader));
    } catch (error) {
      setPlan(null);
      setPlanError(describeIpcError(error));
    } finally {
      setEstimating(false);
    }
  };

  const submit = async () => {
    const created = await create.run(name, versionId, loader);
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
                setTypedName(event.target.value);
              }}
            />
          )}
        </Field>
        <Field
          label={tl.loader}
          hint={loaderKind === 'none' ? tl.hints.none : tl.hints[loaderKind]}
        >
          {(props) => (
            <select
              {...props}
              className={forms.select}
              value={loaderKind}
              onChange={(event) => {
                const kind = event.target.value as LoaderKind | 'none';
                setLoaderKind(kind);
                const known = kind === 'none' ? undefined : gamesByKind[kind];
                if (known !== undefined) keepSupported(known);
                setPlan(null);
                setPlanError(null);
              }}
            >
              <option value="none">{tl.none}</option>
              {LOADER_KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {tl.names[kind]}
                </option>
              ))}
            </select>
          )}
        </Field>
        <fieldset className={forms.choices}>
          <legend>{t.version}</legend>
          {loaderKind !== 'none' && loaderGames !== null && (
            <p className={styles.muted}>{tl.gamesHint({ loader: tl.names[loaderKind] })}</p>
          )}
          {catalog !== null && (
            <VersionPicker
              catalog={catalog}
              value={versionId}
              allowed={loaderKind === 'none' ? null : loaderGames}
              onChange={(version) => {
                setVersionId(version.id);
                setPlan(null);
                setPlanError(null);
              }}
            />
          )}
          <p role="status" className={styles.muted}>
            {versionId === '' ? t.noneSelected : t.selected({ id: versionId })}
          </p>
          <p className={styles.muted}>{t.isolation}</p>
        </fieldset>
        {loaderKind !== 'none' && (
          <Field
            label={tl.version({ loader: tl.names[loaderKind] })}
            hint={
              loaderVersions?.length === 0
                ? tl.noVersions({ loader: tl.names[loaderKind], version: versionId })
                : tl.modsHint
            }
          >
            {(props) => (
              <select
                {...props}
                className={forms.select}
                value={loaderVersion}
                disabled={loaderVersions === null || loaderVersions.length === 0}
                onChange={(event) => {
                  const picked = event.target.value;
                  setPickedByKey((known) => ({ ...known, [loaderKey]: picked }));
                  setPlan(null);
                  setPlanError(null);
                }}
              >
                {loaderVersions === null && <option value="">{tl.loading}</option>}
                {loaderVersions?.map((version) => (
                  <option key={version.id} value={version.id}>
                    {version.recommended
                      ? tl.recommended({ label: version.label })
                      : version.stable
                        ? version.label
                        : tl.unstable({ label: version.label })}
                  </option>
                ))}
              </select>
            )}
          </Field>
        )}
        {shownError !== null && <Notice tone="danger">{shownError}</Notice>}
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
            disabled={estimating || availability !== 'available' || !loaderReady}
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
          <Button
            variant="primary"
            type="submit"
            disabled={create.pending || name.trim() === '' || versionId === '' || !loaderReady}
          >
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
    <section aria-label={t.planTitle({ version: plan.versionId })}>
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
