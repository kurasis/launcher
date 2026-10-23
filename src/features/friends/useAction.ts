import { useState } from 'react';
import { friendsProblem } from '../../lib/ipc';
import { problemText } from './useFriends';

/** Runs one friends action at a time and keeps what went wrong. */
export function useAction() {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async <T>(work: () => Promise<T>): Promise<T | null> => {
    setPending(true);
    setError(null);
    try {
      return await work();
    } catch (e) {
      setError(problemText(friendsProblem(e)));
      return null;
    } finally {
      setPending(false);
    }
  };
  return { pending, error, run };
}
