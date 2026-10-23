import { useState } from 'react';
import type { LocalWorld, RoomConsent, WorldsRoom, WorldsState } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText, worldsMarked } from './useWorlds';

const t = strings.worlds;

export interface RoomTarget {
  /** The room's id: known even before the list of rooms is. */
  roomId: string;
  /** The room as the list has it, when it does. */
  room: WorldsRoom | null;
  /** The room's world when it is kept on this computer. */
  local: LocalWorld | null;
  state: WorldsState;
  /** The local world changed (the EULA was accepted). */
  onLocal: (world: LocalWorld) => void;
}

export type RoomActionsApi = ReturnType<typeof useRoomActions>;

/**
 * Open, close, join, leave and play a room, as this computer can now. A press while a
 * step goes does nothing: the pressed button keeps the focus (a disabled one loses it
 * in WebView2 and WebKit), so buttons show `aria-disabled` while `busy`.
 */
export function useRoomActions({ roomId, room, local, onLocal }: Omit<RoomTarget, 'state'>) {
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [consent, setConsent] = useState<RoomConsent | null>(null);

  /** `said` is false where the state already tells why a step failed. */
  const run = (action: () => Promise<unknown>, said = true) => {
    if (busy) return;
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
    if (busy) return;
    setConsent(null);
    run(async () => {
      if (room !== null && room.pack !== null && room.role !== 'owner') {
        setNote(t.installingMods);
      }
      const instance = await ipc.worldsInstance(roomId, accept);
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
      await ipc.worldsPlay(roomId);
    });
  };

  return {
    busy,
    consent,
    note,
    error,
    play,
    acceptEula: () => {
      if (local === null) return;
      run(async () => {
        onLocal(await ipc.worldsAcceptEula(local.id));
      });
    },
    host: () => {
      run(() => ipc.worldsHost(roomId), false);
    },
    join: () => {
      run(() => ipc.worldsJoin(roomId), false);
    },
    /** A guest whose connection was lost: out, then in again. */
    joinAgain: () => {
      run(() => ipc.worldsDisconnect().then(() => ipc.worldsJoin(roomId)), false);
    },
    stopHosting: () => {
      run(() => ipc.worldsStopHosting());
    },
    disconnect: () => {
      run(() => ipc.worldsDisconnect());
    },
    decline: () => {
      setConsent(null);
    },
  };
}
