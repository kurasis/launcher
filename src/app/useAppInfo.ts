import { useCallback, useEffect, useState } from 'react';
import type { AppInfo } from '../lib/contracts';
import { describeIpcError, ipc } from '../lib/ipc';

export type AppInfoState =
  { status: 'loading' } | { status: 'ready'; info: AppInfo } | { status: 'error'; message: string };

/** Loads the Rust core's build information and exposes a retry action. */
export function useAppInfo(): { state: AppInfoState; reload: () => void } {
  const [state, setState] = useState<AppInfoState>({ status: 'loading' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    ipc.getAppInfo().then(
      (info) => {
        if (active) setState({ status: 'ready', info });
      },
      (error: unknown) => {
        if (active) setState({ status: 'error', message: describeIpcError(error) });
      },
    );
    return () => {
      active = false;
    };
  }, [attempt]);

  const reload = useCallback(() => {
    setState({ status: 'loading' });
    setAttempt((value) => value + 1);
  }, []);

  return { state, reload };
}
