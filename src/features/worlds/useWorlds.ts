import { useEffect, useState } from 'react';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import {
  describeIpcError,
  ipc,
  isLauncherError,
  onWorldsState,
  worldsProblemOf,
} from '../../lib/ipc';
import { strings } from '../../lib/strings';

const IDLE: WorldsState = {
  preparing: null,
  hosting: null,
  guest: null,
  failed: null,
  maintenance: false,
};

/** What this computer does for a room, kept current from `worlds://state`. */
export function useWorldsState(): WorldsState {
  const [state, setState] = useState<WorldsState>(IDLE);
  useEffect(() => {
    let live = true;
    const stop = onWorldsState((next) => {
      setState(next);
    });
    // After the subscription: a later event wins over this first answer.
    void ipc
      .worldsState()
      .then((first) => {
        if (live) setState((current) => (current === IDLE ? first : current));
      })
      .catch(() => undefined);
    return () => {
      live = false;
      stop();
    };
  }, []);
  return state;
}

/** The room this computer gets ready, hosts or is a guest in, if any. */
export function activeRoom(state: WorldsState): string | null {
  return state.preparing?.room ?? state.hosting?.room ?? state.guest?.room ?? null;
}

/** How often rooms are asked again, for their open state and players. */
export const ROOMS_EVERY_MS = 15_000;

/**
 * Whether a room is open and how many play. While this computer hosts it,
 * its own server's count, which is live and includes the host's game; else
 * what worlds.lapi.sh last said.
 */
export function roomOpen(room: WorldsRoom, state: WorldsState): { open: boolean; online: number } {
  const hosting = state.hosting;
  if (hosting?.room === room.id && hosting.ended === null && hosting.phase !== 'exited') {
    return { open: true, online: hosting.players.length };
  }
  return { open: room.hosting, online: room.online };
}

/**
 * How many mods a room plays, when it is known: the owner's own world counts them all;
 * a room open with mods says how many a guest installs (older launchers do not say).
 */
export function roomMods(room: WorldsRoom, world: LocalWorld | null): number | null {
  const count = world !== null && world.loader !== null ? world.mods : room.pack?.mods;
  return count === undefined || count === 0 ? null : count;
}

/**
 * Whether a failure carries this mark: an update of a world's mods failed on
 * its copy, or the world's earlier mods and backup came back (W12).
 */
export function worldsMarked(
  error: unknown,
  mark: 'worldsTrialFailed' | 'worldsRolledBack',
): boolean {
  return isLauncherError(error) && error.safeDetails?.[mark] === true;
}

/** A failure of a worlds command, said in the player's language. */
export function worldsErrorText(error: unknown): string {
  const problem = worldsProblemOf(error);
  const said = problem === null ? describeIpcError(error) : strings.worlds.problems[problem];
  if (worldsMarked(error, 'worldsTrialFailed'))
    return `${strings.worlds.update.trialFailed} ${said}`;
  if (worldsMarked(error, 'worldsRolledBack')) return `${strings.worlds.update.rolledBack} ${said}`;
  return said;
}

/** The `failed` name in the state: a service problem or an error code. */
export function failedText(name: string): string {
  const problems = strings.worlds.problems as Readonly<Record<string, string>>;
  const codes = strings.errorCodes as Readonly<Record<string, string>>;
  return problems[name] ?? codes[name] ?? strings.worlds.problems.refused;
}
