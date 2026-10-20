import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from './useWorlds';
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

  /** `said` is false where the state already tells why a step failed. */
  const run = (action: () => Promise<unknown>, said = true) => {
    setError(null);
    setNote(null);
    setBusy(true);
    action()
      .catch((e: unknown) => {
        if (said) setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const play = () => {
    run(async () => {
      const instance = await ipc.worldsInstance(room.id);
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
            onClick={play}
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
            disabled={busy || state.guest?.connected !== true}
            onClick={play}
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
      {note !== null && <Notice tone="info">{note}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </div>
  );
}
