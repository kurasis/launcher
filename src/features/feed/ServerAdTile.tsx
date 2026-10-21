import { useEffect, useId, useRef, useState } from 'react';
import { type MenuItem, MoreMenu } from '../../app/MoreMenu';
import { StatusBadge } from '../../components/StatusBadge';
import type { AppSnapshot, ServerAd } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { openReview } from '../reviews/openReview';
import { installationOf } from './installations';
import styles from './Feed.module.css';

const t = strings.feed;
const s = strings.feed.serverAd;

const VERSION_KEY = 'serverAd.version.';

/** The version this player picked for an ad before, if the ad still offers it. */
function rememberedVersion(ad: ServerAd): string {
  try {
    const kept = window.localStorage.getItem(VERSION_KEY + ad.id);
    if (kept !== null && ad.versions.includes(kept)) return kept;
  } catch {
    // Storage may be unavailable; the ad's own version is used.
  }
  return ad.version;
}

function rememberVersion(ad: ServerAd, version: string) {
  try {
    if (version === ad.version) window.localStorage.removeItem(VERSION_KEY + ad.id);
    else window.localStorage.setItem(VERSION_KEY + ad.id, version);
  } catch {
    // Not remembered: the next start offers the ad's own version.
  }
}

/** A vanilla installation of `version`: Play joins servers without mods. */
function vanillaOf(snapshot: AppSnapshot, version: string) {
  return installationOf(
    snapshot.instances.filter((instance) => instance.loader === null),
    version,
  );
}

type Stage = 'idle' | 'creating' | 'installing' | 'starting';

/**
 * An ad for a Minecraft server (D-069), shaped like the feed's other tiles:
 * the server's icon as the picture, with the label, players online and Play
 * on it; the name, the text and the address under it. Play installs the
 * chosen version when no vanilla installation of it is ready, then starts the
 * game straight into the server. The ⋯ menu picks another version the server
 * lets in, copies the address and reports the ad.
 */
export function ServerAdTile({ ad, snapshot }: { ad: ServerAd; snapshot: AppSnapshot }) {
  const [version, setVersion] = useState(() => rememberedVersion(ad));
  const [versions, setVersions] = useState(false);
  const [stage, setStage] = useState<Stage>('idle');
  /** The installation being installed, to join once it is ready. */
  const [waiting, setWaiting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const requestId = useRef<string | null>(null);
  /** The install was seen running, so its end is its result. */
  const seen = useRef(false);
  /** The install's end is being handled: once. */
  const finishing = useRef(false);
  const versionsId = useId();
  const versionsRef = useRef<HTMLDivElement>(null);
  const playButton = useRef<HTMLButtonElement>(null);

  const instance = vanillaOf(snapshot, version);
  const ready = instance?.health === 'ready';
  const running = snapshot.currentRun !== null;
  const inGame = running && snapshot.currentRun?.instanceId === instance?.id && stage === 'idle';

  const join = async (instanceId: string) => {
    setStage('starting');
    requestId.current ??= crypto.randomUUID();
    try {
      await ipc.launchGame(instanceId, requestId.current, ad.address);
      requestId.current = null;
    } catch (failure) {
      requestId.current = null;
      setError(describeIpcError(failure));
    } finally {
      setStage('idle');
    }
  };

  // Joins once the installation started for it is ready; says so if the
  // install ended without making it ready.
  const waited = waiting === null ? null : snapshot.instances.find((i) => i.id === waiting);
  const installing =
    waiting !== null &&
    (snapshot.activeOperation?.instanceId === waiting ||
      snapshot.queue.some((queued) => queued.instanceId === waiting));
  useEffect(() => {
    if (waiting === null) return;
    if (installing) {
      seen.current = true;
      return;
    }
    const done = waited?.health === 'ready';
    if ((!done && !seen.current) || finishing.current) return;
    finishing.current = true;
    // After this render: the install's end is news from the core.
    void Promise.resolve().then(() => {
      setWaiting(null);
      if (done) {
        void join(waiting);
      } else {
        setStage('idle');
        setError(s.installFailed({ version }));
      }
    });
    // `join` changes every render; the effect runs on what the install did.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [waiting, installing, waited?.health]);

  const play = async () => {
    setError(null);
    setVersions(false);
    if (ready) {
      await join(instance.id);
      return;
    }
    try {
      let id = instance?.id ?? null;
      if (id === null) {
        setStage('creating');
        const created = await ipc.createInstance({
          displayName: t.versionTitle({ version }),
          versionId: version,
          loader: null,
        });
        id = created.id;
      }
      setStage('installing');
      seen.current = false;
      finishing.current = false;
      await ipc.installInstance(id);
      setWaiting(id);
    } catch (failure) {
      setStage('idle');
      setError(describeIpcError(failure));
    }
  };

  // The versions open on the chosen one; Escape and a click outside close them.
  useEffect(() => {
    if (!versions) return;
    versionsRef.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]')?.focus();
    const close = (event: PointerEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent) {
        if (event.key !== 'Escape') return;
        setVersions(false);
        playButton.current?.focus();
        return;
      }
      if (event.target instanceof Node && versionsRef.current?.contains(event.target)) return;
      setVersions(false);
    };
    document.addEventListener('pointerdown', close, true);
    document.addEventListener('keydown', close);
    return () => {
      document.removeEventListener('pointerdown', close, true);
      document.removeEventListener('keydown', close);
    };
  }, [versions]);

  const pick = (choice: string) => {
    setVersion(choice);
    rememberVersion(ad, choice);
    setVersions(false);
    setError(null);
    playButton.current?.focus();
  };

  const copy = () => {
    void navigator.clipboard
      .writeText(ad.address)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => {
          setCopied(false);
        }, 1500);
      })
      .catch(() => undefined);
  };

  const report = () => {
    const id = Number(ad.id);
    openReview({
      kind: 'bug',
      category: 'feed',
      subcategory: 'server_ads',
      ad: Number.isSafeInteger(id) && id > 0 ? id : null,
    });
  };

  const players =
    ad.up === false ? s.downShort : ad.online === null ? null : s.players({ online: ad.online });
  const playersInFull =
    ad.up === false
      ? s.down
      : ad.online === null
        ? null
        : ad.max === null
          ? s.onlineOnly({ online: ad.online })
          : s.online({ online: ad.online, max: ad.max });
  const label =
    stage === 'creating' || stage === 'installing' || waiting !== null
      ? s.installing({ version })
      : stage === 'starting'
        ? s.starting
        : inGame
          ? s.inGame
          : s.play;
  const hint = ready ? s.installed({ version }) : s.toDownload({ version });
  const working = stage !== 'idle' || waiting !== null;
  const oldest = ad.versions[ad.versions.length - 1] ?? ad.version;
  const newest = ad.versions[0] ?? ad.version;

  const entries: MenuItem[] = [];
  if (ad.versions.length > 1 && !working) {
    entries.push({
      kind: 'item',
      key: 'version',
      label: s.versionItem({ version }),
      onSelect: () => {
        setVersions(true);
      },
    });
  }
  entries.push(
    { kind: 'item', key: 'copy', label: s.copy({ address: ad.address }), onSelect: copy },
    { kind: 'separator', key: 'end' },
    { kind: 'item', key: 'report', label: s.report, onSelect: report },
  );

  return (
    <article className={styles.tile} aria-label={`${s.label}: ${ad.name}`}>
      <span className={styles.tileMenu}>
        <MoreMenu
          label="⋯"
          ariaLabel={s.menu({ name: ad.name })}
          buttonClassName={styles.tileMenuButton}
          caret={false}
          current={false}
          entries={entries}
        />
      </span>
      {versions && (
        <div
          ref={versionsRef}
          id={versionsId}
          className={styles.adMenu}
          role="menu"
          aria-label={s.chooseVersion({ name: ad.name })}
        >
          <p className={styles.muted}>{s.range({ from: oldest, to: newest })}</p>
          {ad.versions.map((choice) => {
            const mark =
              choice === ad.version
                ? s.serverVersion
                : vanillaOf(snapshot, choice)?.health === 'ready'
                  ? s.installedMark
                  : '';
            return (
              <button
                key={choice}
                type="button"
                role="menuitemradio"
                aria-checked={choice === version}
                className={styles.adMenuItem}
                onClick={() => {
                  pick(choice);
                }}
              >
                <span>{choice}</span>
                <span className={styles.muted}>{mark}</span>
              </button>
            );
          })}
        </div>
      )}
      <div className={[styles.preview, styles.adPreview].join(' ')}>
        {ad.icon !== null ? (
          <>
            <img className={styles.adBackdrop} src={ad.icon} alt="" aria-hidden="true" />
            <span className={styles.adShade} aria-hidden="true" />
            <img className={styles.adIcon} src={ad.icon} alt="" />
          </>
        ) : (
          <span className={styles.cover} dir="auto">
            {ad.name}
          </span>
        )}
        <span className={styles.overlayBadge}>
          <StatusBadge tone="neutral" label={s.label} />
        </span>
        <div className={styles.adBar}>
          {players !== null && (
            <span className={styles.adOnline} title={playersInFull ?? undefined}>
              <span
                className={ad.up === false ? styles.adDotOff : styles.adDot}
                aria-hidden="true"
              />
              <span aria-hidden="true">{players}</span>
              <span className={styles.visuallyHidden}>{playersInFull}</span>
            </span>
          )}
          <button
            ref={playButton}
            type="button"
            className={styles.adPlay}
            aria-label={`${label}, ${hint}`}
            title={hint}
            disabled={working || running}
            aria-controls={versions ? versionsId : undefined}
            onClick={() => void play()}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true" focusable="false">
              <path d="M1 0 9 5 1 10z" />
            </svg>
            {label}
          </button>
        </div>
      </div>
      <div className={styles.tileText}>
        <h3 className={styles.tileTitle} dir="auto">
          {ad.name}
        </h3>
        <p className={styles.tileLine} dir="auto">
          {ad.text}
        </p>
        <p className={styles.tileLine} role={copied ? 'status' : undefined}>
          {copied ? (
            s.copied
          ) : (
            <>
              <span dir="ltr">{ad.address}</span> · {version}
            </>
          )}
        </p>
        {ad.erid !== null && <p className={styles.tileLine}>{s.erid({ erid: ad.erid })}</p>}
        {running && !inGame && stage === 'idle' && (
          <p className={styles.tileLine} role="status">
            {t.gameRunning}
          </p>
        )}
        {error !== null && (
          <p className={styles.adError} role="alert">
            {error}
          </p>
        )}
      </div>
    </article>
  );
}
