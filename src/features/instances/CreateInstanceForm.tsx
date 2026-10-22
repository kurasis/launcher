import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
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
import { recommendationText, useRecommendation } from '../settings/memoryLabel';
import styles from './CreateInstance.module.css';
import { LoaderPills } from './LoaderPills';
import { VersionPicker } from './VersionPicker';

interface CreateInstanceFormProps {
  /** Closes the page; with the new installation's ID after creating one. */
  onDone: (createdId?: string) => void;
  /** When the connection returns, the version list is fetched again (D-052). */
  network?: NetworkStatus;
  /** The fixed memory for new installations; null uses the recommendation (D-058). */
  fixedHeapMib?: number | null;
  /** The version chosen first, when the catalog has it (the feed's Install). */
  initialVersion?: string | null;
  /** The mod loader chosen first (the command palette's "New installation with …"). */
  initialLoader?: LoaderKind | null | undefined;
}

const t = strings.createForm;
const tl = strings.loaders;
/** The core accepts 1–64 characters (src-tauri/src/validation.rs). */
const NAME_MAX_LENGTH = 64;

/** The default name: the version, and the loader when there is one. */
function defaultName(versionId: string, kind: LoaderKind | 'none'): string {
  return kind === 'none' ? versionId : `${versionId} ${tl.names[kind]}`;
}

const availabilityTone: Record<VersionAvailability, StatusTone> = {
  available: 'success',
  not_in_catalog: 'danger',
  catalog_unavailable: 'warning',
};

/**
 * The new-installation page (D-072): the way back and the title, then one card
 * with the name, mod loader and memory beside the Minecraft version.
 */
export function CreateInstanceForm({
  onDone,
  network = 'online',
  fixedHeapMib = null,
  initialVersion = null,
  initialLoader = null,
}: CreateInstanceFormProps) {
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
  const [loaderKind, setLoaderKind] = useState<LoaderKind | 'none'>(initialLoader ?? 'none');
  const [gamesByKind, setGamesByKind] = useState<Partial<Record<LoaderKind, ReadonlySet<string>>>>(
    {},
  );
  const [versionsByKey, setVersionsByKey] = useState<Record<string, LoaderVersion[]>>({});
  const [pickedByKey, setPickedByKey] = useState<Record<string, string>>({});
  const [loaderError, setLoaderError] = useState<{ key: string; message: string } | null>(null);
  // Bumped by Try again: the loader requests run again for what is not cached.
  const [loaderAttempt, setLoaderAttempt] = useState(0);
  const create = useCommand(
    (requestId: string, displayName: string, version: string, loader: LoaderChoice | null) =>
      ipc.createInstance({ displayName, versionId: version, loader }, requestId),
  );
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);

  // The page replaces what was on screen: start reading it from its title.
  useEffect(() => {
    headingRef.current?.focus();
  }, []);

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
  // The chosen game version is one the loader does not support, or none is.
  const unsupported = loaderGames !== null && versionId !== '' && !loaderGames.has(versionId);
  const noneSupported =
    loaderGames !== null &&
    catalog !== null &&
    !catalog.versions.some((version) => loaderGames.has(version.id));

  // The backend reports catalog outages inside the view; a rejection only means
  // the core did not answer, which the availability badge shows as unreachable.
  // Only the latest request's answer counts: Check again, the connection's return
  // and the first load can finish in any order.
  const catalogAsked = useRef(0);
  const fetchCatalog = useCallback(
    (refresh: boolean) => {
      const asked = ++catalogAsked.current;
      return ipc
        .listSupportedVersions(refresh)
        .then((view) => {
          if (asked !== catalogAsked.current) return;
          setCatalog(view);
          // Preselect the version asked for, else the latest release.
          const initial =
            view.versions.find((v) => initialVersion !== null && v.id === initialVersion) ??
            view.versions.find((v) => v.id === view.latestRelease) ??
            view.versions[0];
          if (initial !== undefined) {
            setVersionId((current) => (current === '' ? initial.id : current));
          }
        })
        .catch(() => {
          if (asked === catalogAsked.current) setCatalog(null);
        })
        .finally(() => {
          if (asked === catalogAsked.current) setChecking(false);
        });
    },
    [initialVersion],
  );

  useEffect(() => {
    void fetchCatalog(false);
  }, [fetchCatalog]);

  const loadCatalog = (refresh: boolean) => {
    setChecking(true);
    void fetchCatalog(refresh);
  };

  // Back online: check the version list again, and say so while it runs.
  const wasOffline = useRef(network === 'offline');
  useEffect(() => {
    if (network === 'online' && wasOffline.current) {
      setChecking(true);
      void fetchCatalog(true);
    }
    wasOffline.current = network === 'offline';
  }, [network, fetchCatalog]);

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
        setLoaderError((error) => (error?.key === loaderKind ? null : error));
        keepSupported(allowed);
      })
      .catch((error: unknown) => {
        if (current) setLoaderError({ key: loaderKind, message: describeIpcError(error) });
      });
    return () => {
      current = false;
    };
  }, [loaderKind, gamesByKind, keepSupported, loaderAttempt]);

  // The loader's list can arrive before the catalog (a loader chosen from the
  // command palette): the selection moves once both are there.
  const [catalogSeen, setCatalogSeen] = useState<CatalogView | null>(null);
  if (catalog !== catalogSeen) {
    setCatalogSeen(catalog);
    if (catalog !== null && loaderGames !== null) keepSupported(loaderGames);
  }

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
        if (!current) return;
        setVersionsByKey((known) => ({ ...known, [loaderKey]: versions }));
        setLoaderError((error) => (error?.key === loaderKey ? null : error));
      })
      .catch((error: unknown) => {
        if (current) setLoaderError({ key: loaderKey, message: describeIpcError(error) });
      });
    return () => {
      current = false;
    };
  }, [loaderKind, versionId, loaderGames, loaderKey, versionsByKey, loaderAttempt]);

  /** Asks again for what failed; nothing failed is cached, so it is fetched anew. */
  const retryLoader = () => {
    setLoaderError(null);
    setLoaderAttempt((attempt) => attempt + 1);
  };

  const selected = catalog?.versions.find((version) => version.id === versionId);
  const availability = selected?.availability ?? 'catalog_unavailable';
  const loaderReady = loaderKind === 'none' || loader !== null;
  const loaderLoading =
    loaderKind !== 'none' && loaderVersions === null && shownError === null && !unsupported;

  // An estimate answers for the version and loader it was asked for: one that
  // arrives after either changed is dropped. The effect counts only a real
  // change, so an effect that runs late, after a click on Estimate for the same
  // choice, does not drop that estimate.
  const planFor = JSON.stringify([versionId, loaderKind, loaderVersion]);
  const planAsked = useRef({ count: 0, planFor: '' });
  useEffect(() => {
    if (planAsked.current.planFor !== planFor) {
      planAsked.current = { count: planAsked.current.count + 1, planFor };
    }
  }, [planFor]);

  const estimate = async () => {
    const asked = planAsked.current.count + 1;
    planAsked.current = { count: asked, planFor };
    setEstimating(true);
    setPlanError(null);
    try {
      const answer = await ipc.previewInstallPlan(versionId, loader);
      if (asked === planAsked.current.count) setPlan(answer);
    } catch (error) {
      if (asked === planAsked.current.count) {
        setPlan(null);
        setPlanError(describeIpcError(error));
      }
    } finally {
      setEstimating(false);
    }
  };

  const submit = async () => {
    const created = await create.run(name, versionId, loader);
    if (created) onDone(created.id);
  };

  return (
    <section className={styles.page} aria-labelledby={headingId}>
      <header className={styles.header}>
        <button
          type="button"
          className={styles.back}
          aria-label={t.back}
          title={t.back}
          onClick={() => {
            onDone();
          }}
        >
          <Icon name="back" size={18} className={styles.backIcon} />
        </button>
        <h2 id={headingId} ref={headingRef} tabIndex={-1} className={styles.title}>
          {t.title}
        </h2>
      </header>
      <form
        className={styles.card}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <div className={styles.columns}>
          <div className={styles.column}>
            <Field label={t.name} hint={t.nameHint}>
              {(props) => (
                <input
                  {...props}
                  className={forms.input}
                  value={name}
                  maxLength={NAME_MAX_LENGTH}
                  required
                  autoComplete="off"
                  spellCheck={false}
                  // "26.3 Fabric" reads left to right in Arabic too; an Arabic name right to left.
                  dir="auto"
                  onChange={(event) => {
                    setTypedName(event.target.value.slice(0, NAME_MAX_LENGTH));
                  }}
                />
              )}
            </Field>
            <LoaderPills
              value={loaderKind}
              onChange={(kind) => {
                setLoaderKind(kind);
                const known = kind === 'none' ? undefined : gamesByKind[kind];
                if (known !== undefined) keepSupported(known);
                setPlan(null);
                setPlanError(null);
              }}
            />
            {loaderKind !== 'none' && (
              <div className={styles.stack}>
                <Field
                  label={tl.version({ loader: tl.names[loaderKind] })}
                  hint={
                    unsupported || loaderVersions?.length === 0
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
                      {loaderVersions === null && (
                        <option value="">{loaderLoading ? tl.loading : ''}</option>
                      )}
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
                {shownError !== null && (
                  <>
                    <Notice tone="danger">{shownError}</Notice>
                    <div className={styles.row}>
                      <Button onClick={retryLoader}>
                        <Icon name="refresh" size={16} />
                        {t.retry}
                      </Button>
                    </div>
                  </>
                )}
              </div>
            )}
            {versionId !== '' && (
              <MemoryLine
                versionId={versionId}
                modded={loaderKind !== 'none'}
                fixedHeapMib={fixedHeapMib}
              />
            )}
          </div>
          <div className={styles.column}>
            <div className={styles.stack}>
              <fieldset className={styles.group}>
                <legend className={styles.legend}>{t.version}</legend>
                {loaderKind !== 'none' && noneSupported ? (
                  <Notice tone="info">{t.noSupported({ loader: tl.names[loaderKind] })}</Notice>
                ) : (
                  catalog !== null && (
                    <VersionPicker
                      catalog={catalog}
                      value={versionId}
                      allowed={loaderKind === 'none' ? null : loaderGames}
                      note={
                        loaderKind !== 'none' && loaderGames !== null
                          ? tl.gamesHint({ loader: tl.names[loaderKind] })
                          : null
                      }
                      onChange={(version) => {
                        setVersionId(version.id);
                        setPlan(null);
                        setPlanError(null);
                      }}
                    />
                  )
                )}
              </fieldset>
              <div className={styles.chosenRow}>
                <p role="status" className={styles.chosen}>
                  <span>{versionId === '' ? t.noneSelected : t.selected({ id: versionId })}</span>{' '}
                  <span className={styles.separator} aria-hidden="true">
                    ·
                  </span>{' '}
                  <span>
                    {checking ? (
                      t.availability.checking
                    ) : (
                      <StatusBadge
                        tone={availabilityTone[availability]}
                        label={t.availability[availability]}
                      />
                    )}
                  </span>
                </p>
                <Button
                  disabled={checking}
                  onClick={() => {
                    loadCatalog(true);
                  }}
                >
                  <Icon name="refresh" size={16} />
                  {t.refresh}
                </Button>
              </div>
              <div className={styles.row}>
                <Button
                  disabled={estimating || availability !== 'available' || !loaderReady}
                  onClick={() => void estimate()}
                >
                  {estimating ? t.estimating : t.estimate}
                </Button>
              </div>
            </div>
            {catalog?.stale === true && <Notice tone="warning">{t.stale}</Notice>}
            {planError !== null && <Notice tone="danger">{planError}</Notice>}
            {plan !== null && <PlanFacts plan={plan} />}
          </div>
        </div>
        <div className={styles.footer}>
          <p className={styles.isolation}>{t.isolation}</p>
          {create.error !== null && <Notice tone="danger">{create.error}</Notice>}
          <div className={styles.row}>
            <Button
              variant="primary"
              type="submit"
              disabled={create.pending || name.trim() === '' || versionId === '' || !loaderReady}
            >
              {t.submit}
            </Button>
            <Button
              onClick={() => {
                onDone();
              }}
            >
              {t.cancel}
            </Button>
          </div>
        </div>
      </form>
    </section>
  );
}

/** The memory the new installation gets: the fixed default or the recommendation. */
function MemoryLine({
  versionId,
  modded,
  fixedHeapMib,
}: {
  versionId: string;
  modded: boolean;
  fixedHeapMib: number | null;
}) {
  const recommendation = useRecommendation(fixedHeapMib === null ? { versionId, modded } : null);
  const text =
    fixedHeapMib !== null
      ? t.memoryFixed({ mib: fixedHeapMib })
      : recommendation === null
        ? null
        : recommendationText(recommendation);
  if (text === null) return null;
  return (
    <div className={styles.block}>
      <p className={styles.label}>{t.memory}</p>
      <p>{text}</p>
      <p className={styles.muted}>{t.memoryHint}</p>
    </div>
  );
}

function PlanFacts({ plan }: { plan: PlanSummary }) {
  return (
    <section className={styles.plan} aria-label={t.planTitle({ version: plan.versionId })}>
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
