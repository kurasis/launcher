import { useEffect, useState } from 'react';
import type { WorldsState } from '../../lib/contracts';
import { describeIpcError, ipc, onWorldsState, worldsProblemOf } from '../../lib/ipc';
import { strings } from '../../lib/strings';

const IDLE: WorldsState = { preparing: null, hosting: null, guest: null, failed: null };

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

/** A failure of a worlds command, said in the player's language. */
export function worldsErrorText(error: unknown): string {
  const problem = worldsProblemOf(error);
  return problem === null ? describeIpcError(error) : strings.worlds.problems[problem];
}

/** The `failed` name in the state: a service problem or an error code. */
export function failedText(name: string): string {
  const problems = strings.worlds.problems as Readonly<Record<string, string>>;
  const codes = strings.errorCodes as Readonly<Record<string, string>>;
  return problems[name] ?? codes[name] ?? strings.worlds.problems.refused;
}
