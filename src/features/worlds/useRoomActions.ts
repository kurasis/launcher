import { useEffect, useRef, useState } from 'react';
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
 * in WebView2 and WebKit), so buttons show `aria-disabled` while `busy`. The ways out
 * (Leave the world, Join again, Close the world) wait only for each other: a Play that
 * installs mods does not keep the player in the room. Another `roomId` (the band follows
 * the room this computer is in) starts afresh, and what was under way for the last one
 * is left undone.
 */
export function useRoomActions({ roomId, room, local, onLocal }: Omit<RoomTarget, 'state'>) {
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [consent, setConsent] = useState<RoomConsent | null>(null);
  const [target, setTarget] = useState(roomId);
  if (target !== roomId) {
    setTarget(roomId);
    setError(null);
    setNote(null);
    setBusy(false);
    setLeaving(false);
    setConsent(null);
  }
  // Counts the rooms followed and the ways out taken: what a step started before
  // either changed is left undone.
  const rooms = useRef(0);
  const ways = useRef(0);
  useEffect(() => {
    rooms.current += 1;
  }, [roomId]);

  /** `said` is false where the state already tells why a step failed. */
  const run = (action: (stale: () => boolean) => Promise<unknown>, said = true) => {
    if (busy) return;
    const room0 = rooms.current;
    const way0 = ways.current;
    const stale = () => rooms.current !== room0 || ways.current !== way0;
    setError(null);
    setNote(null);
    setBusy(true);
    action(stale)
      .catch((e: unknown) => {
        if (stale()) return;
        setNote(null);
        // The state names a failed start, but not that the mods went back.
        if (said || worldsMarked(e, 'worldsRolledBack')) setError(worldsErrorText(e));
      })
      .finally(() => {
        if (rooms.current === room0) setBusy(false);
      });
  };

  /** A way out: it ends the question about the mods and what Play was doing. */
  const leave = (action: () => Promise<unknown>, said = true) => {
    if (leaving) return;
    const room0 = rooms.current;
    ways.current += 1;
    setConsent(null);
    setError(null);
    setNote(null);
    setLeaving(true);
    action()
      .catch((e: unknown) => {
        if (said && rooms.current === room0) setError(worldsErrorText(e));
      })
      .finally(() => {
        if (rooms.current === room0) setLeaving(false);
      });
  };

  /** `accept` is the room pack the player agreed to install. */
  const play = (accept?: string) => {
    if (busy || leaving) return;
    setConsent(null);
    run(async (stale) => {
      if (room !== null && room.pack !== null && room.role !== 'owner') {
        setNote(t.installingMods);
      }
      const instance = await ipc.worldsInstance(roomId, accept);
      if (stale()) return;
      setNote(null);
      if (instance.consent !== null) {
        setConsent(instance.consent);
        return;
      }
      if (!instance.ready) {
        await ipc.installInstance(instance.instanceId);
        if (stale()) return;
        setNote(t.installing);
        return;
      }
      await ipc.worldsPlay(roomId);
    });
  };

  return {
    busy,
    leaving,
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
      leave(() => ipc.worldsDisconnect().then(() => ipc.worldsJoin(roomId)), false);
    },
    stopHosting: () => {
      leave(() => ipc.worldsStopHosting());
    },
    disconnect: () => {
      leave(() => ipc.worldsDisconnect());
    },
    decline: () => {
      if (busy) return;
      setConsent(null);
    },
  };
}
