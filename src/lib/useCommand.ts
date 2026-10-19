import { useCallback, useRef, useState } from 'react';
import { describeIpcError, isLauncherError } from './ipc';
import { appStore } from './store';

export interface CommandState {
  pending: boolean;
  error: string | null;
}

/**
 * Runs a backend command, tracks its pending state and keeps the request ID stable
 * across retries of the same logical action until it succeeds, so a retried IPC
 * call is deduplicated by the backend instead of applied twice.
 */
export function useCommand<A extends unknown[], R>(
  command: (requestId: string, ...args: A) => Promise<R>,
): CommandState & { run: (...args: A) => Promise<R | undefined>; reset: () => void } {
  const [state, setState] = useState<CommandState>({ pending: false, error: null });
  const requestId = useRef<string | null>(null);
  const lastArgs = useRef<string | null>(null);

  const run = useCallback(
    async (...args: A): Promise<R | undefined> => {
      const fingerprint = JSON.stringify(args);
      if (requestId.current === null || lastArgs.current !== fingerprint) {
        requestId.current = crypto.randomUUID();
        lastArgs.current = fingerprint;
      }
      setState({ pending: true, error: null });
      try {
        const result = await command(requestId.current, ...args);
        requestId.current = null;
        setState({ pending: false, error: null });
        // Events normally update the store; a refresh covers a missed delivery.
        appStore.refresh();
        return result;
      } catch (error) {
        // A typed rejection is final for this request ID; only transport failures
        // keep the ID so a retry is recognized by the backend.
        if (isLauncherError(error)) {
          requestId.current = null;
        }
        setState({ pending: false, error: describeIpcError(error) });
        return undefined;
      }
    },
    [command],
  );

  const reset = useCallback(() => {
    setState({ pending: false, error: null });
  }, []);

  return { ...state, run, reset };
}
