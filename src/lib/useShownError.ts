import { useCallback, useState } from 'react';
import { describeIpcError } from './ipc';

/**
 * The failure of a one-off call (a file picker, opening a link), for the
 * component that made it to show: `catcher` goes in `.catch()`, `clear`
 * before trying again.
 */
export function useShownError(): {
  error: string | null;
  catcher: (error: unknown) => void;
  clear: () => void;
} {
  const [error, setError] = useState<string | null>(null);
  const catcher = useCallback((e: unknown) => {
    setError(describeIpcError(e));
  }, []);
  const clear = useCallback(() => {
    setError(null);
  }, []);
  return { error, catcher, clear };
}
