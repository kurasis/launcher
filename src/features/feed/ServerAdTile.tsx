import { useEffect, useId, useRef, useState } from 'react';
import type { AppSnapshot, ServerAd } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
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
 * An ad for a Minecraft server (D-069): its icon, name, players online, text
 * and address. Play installs the chosen version when no vanilla installation
 * of it is ready, then starts the game straight into the server; the button
 * beside it picks another version the server lets in.
 */
export function ServerAdTile({ ad, snapshot }: { ad: ServerAd; snapshot: AppSnapshot }) {
  const [version, setVersion] = useState(() => rememberedVersion(ad));
  const [menu, setMenu] = useState(false);
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
  const menuId = useId();
  const menuRef = useRef<HTMLDivElement>(null);
  const versionButton = useRef<HTMLButtonElement>(null);

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
    setMenu(false);
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

  // The menu closes on Escape and on a click outside it.
  useEffect(() => {
    if (!menu) return;
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent) {
        if (event.key !== 'Escape') return;
        setMenu(false);
        versionButton.current?.focus();
        return;
      }
      const target = event.target as Node;
      if (!menuRef.current?.contains(target) && !versionButton.current?.contains(target)) {
        setMenu(false);
      }
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', close);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', close);
    };
  }, [menu]);

  const pick = (choice: string) => {
    setVersion(choice);
    rememberVersion(ad, choice);
    setMenu(false);
    setError(null);
    versionButton.current?.focus();
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

  const players =
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
  const line = ready ? s.installed({ version }) : s.toDownload({ version });
  const working = stage !== 'idle' || waiting !== null;
  const oldest = ad.versions[ad.versions.length - 1] ?? ad.version;
  const newest = ad.versions[0] ?? ad.version;

  return (
    <article className={styles.adTile} aria-label={`${s.label}: ${ad.name}`}>
      <span className={styles.adLabel}>{s.label}</span>
      <div className={styles.adHead}>
        {ad.icon !== null ? (
          <img className={styles.adIcon} src={ad.icon} alt="" />
        ) : (
          <span className={styles.adIcon} aria-hidden="true" />
        )}
        <div className={styles.adName}>
          <h3 className={styles.tileTitle} dir="auto">
            {ad.name}
          </h3>
          {players !== null && (
            <p className={styles.muted}>
              <span
                className={ad.up === false ? styles.adDotOff : styles.adDot}
                aria-hidden="true"
              />{' '}
              {players}
            </p>
          )}
        </div>
      </div>
      <p className={styles.adText} dir="auto">
        {ad.text}
      </p>
      <button
        type="button"
        className={styles.adAddress}
        aria-label={s.copy({ address: ad.address })}
        onClick={copy}
      >
        <span dir="ltr">{ad.address}</span>
        <span className={styles.muted}>{copied ? s.copied : ''}</span>
      </button>
      <div className={styles.adActions}>
        <button
          type="button"
          className={styles.adPlay}
          disabled={working || running}
          onClick={() => void play()}
        >
          <span>{label}</span>
          <small>{line}</small>
        </button>
        {ad.versions.length > 1 && (
          <button
            ref={versionButton}
            type="button"
            className={styles.adVersion}
            aria-haspopup="menu"
            aria-expanded={menu}
            aria-controls={menu ? menuId : undefined}
            aria-label={s.chooseVersion({ name: ad.name })}
            title={s.chooseVersion({ name: ad.name })}
            disabled={working}
            onClick={() => {
              setMenu((open) => !open);
            }}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
              <path d="M7 4v16M7 20l-3-3M7 20l3-3M17 20V4M17 4l-3 3M17 4l3 3" />
            </svg>
          </button>
        )}
      </div>
      {menu && (
        <div ref={menuRef} id={menuId} className={styles.adMenu} role="menu">
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
      {running && !inGame && stage === 'idle' && <p role="status">{t.gameRunning}</p>}
      {ad.erid !== null && <p className={styles.muted}>{s.erid({ erid: ad.erid })}</p>}
      {error !== null && (
        <p className={styles.adError} role="alert">
          {error}
        </p>
      )}
    </article>
  );
}
