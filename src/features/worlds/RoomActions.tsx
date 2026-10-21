import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { LocalWorld, RoomConsent, WorldsRoom, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText, worldsMarked } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  room: WorldsRoom;
  /** The room's world when it is kept on this computer. */
  local: LocalWorld | null;
  state: WorldsState;
  /** The local world changed (the EULA was accepted). */
  onLocal: (world: LocalWorld) => void;
}

/** Open, close, join, leave and play a room, as this computer can now. */
export function RoomActions({ room, local, state, onLocal }: Props) {
  const t = strings.worlds;
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [consent, setConsent] = useState<RoomConsent | null>(null);

  /** `said` is false where the state already tells why a step failed. */
  const run = (action: () => Promise<unknown>, said = true) => {
    setError(null);
    setNote(null);
    setBusy(true);
    action()
      .catch((e: unknown) => {
        setNote(null);
        // The state names a failed start, but not that the mods went back.
        if (said || worldsMarked(e, 'worldsRolledBack')) setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  /** `accept` is the room pack the player agreed to install. */
  const play = (accept?: string) => {
    setConsent(null);
    run(async () => {
      if (room.pack !== null && room.role !== 'owner') setNote(t.installingMods);
      const instance = await ipc.worldsInstance(room.id, accept);
      setNote(null);
      if (instance.consent !== null) {
        setConsent(instance.consent);
        return;
      }
      if (!instance.ready) {
        await ipc.installInstance(instance.instanceId);
        setNote(t.installing);
        return;
      }
      await ipc.worldsPlay(room.id);
    });
  };

  const inRoom = state.preparing !== null || state.hosting !== null || state.guest !== null;
  const hostingHere = state.hosting?.room === room.id && state.hosting.ended === null;
  const guestHere = state.guest?.room === room.id;
  const owner = room.role === 'owner';

  return (
    <div className={styles.actions}>
      {owner && local !== null && !local.eulaAccepted && (
        <>
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => {
              run(async () => {
                onLocal(await ipc.worldsAcceptEula(local.id));
              });
            }}
          >
            {t.acceptEula}
          </Button>
          <Button
            onClick={() => {
              void ipc.openExternal('minecraft_eula').catch(() => undefined);
            }}
          >
            {t.readEula}
          </Button>
        </>
      )}
      {owner && local?.eulaAccepted === true && !inRoom && (
        <Button
          variant="primary"
          disabled={busy || state.maintenance}
          onClick={() => {
            run(() => ipc.worldsHost(room.id), false);
          }}
        >
          {t.host}
        </Button>
      )}
      {hostingHere && (
        <>
          <Button
            variant="primary"
            disabled={busy || state.hosting?.phase !== 'ready'}
            onClick={() => {
              play();
            }}
          >
            {t.play}
          </Button>
          <Button
            disabled={busy}
            onClick={() => {
              run(() => ipc.worldsStopHosting());
            }}
          >
            {t.stopHosting}
          </Button>
        </>
      )}
      {!owner && room.hosting && !inRoom && (
        <Button
          variant="primary"
          disabled={busy || state.maintenance}
          onClick={() => {
            run(() => ipc.worldsJoin(room.id), false);
          }}
        >
          {t.join}
        </Button>
      )}
      {guestHere && (
        <>
          <Button
            variant="primary"
            disabled={busy || state.guest?.connected !== true || consent !== null}
            onClick={() => {
              play();
            }}
          >
            {t.play}
          </Button>
          <Button
            disabled={busy}
            onClick={() => {
              run(() => ipc.worldsDisconnect());
            }}
          >
            {t.disconnect}
          </Button>
        </>
      )}
      {owner && local === null && <span className={styles.muted}>{t.keptElsewhere}</span>}
      {owner && local !== null && !local.eulaAccepted && (
        <span className={styles.muted}>{t.eulaNeeded}</span>
      )}
      {consent !== null && (
        <Notice tone="warning">
          <strong>{t.consent.title({ mods: consent.mods })}</strong>
          <p>
            {t.consent.new({ count: consent.newMods, size: formatBytes(consent.bytes) })}{' '}
            {consent.names.join(', ')}
            {consent.newMods > consent.names.length &&
              ` ${t.consent.more({ count: consent.newMods - consent.names.length })}`}
          </p>
          <p>{t.consent.warning}</p>
          <div className={styles.actions}>
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => {
                play(consent.sha256);
              }}
            >
              {t.consent.accept}
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                setConsent(null);
              }}
            >
              {t.consent.decline}
            </Button>
          </div>
        </Notice>
      )}
      {note !== null && <Notice tone="info">{note}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </div>
  );
}
