import { useEffect, useState } from 'react';
import type { MicrosoftAccount } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { isPngDataUrl } from '../skins/textures';

/** Heads by profile ID, kept with the accounts they were asked for. */
interface Heads {
  for: string;
  heads: ReadonlyMap<string, string>;
}

/** A head missing after Mojang could not be reached is asked for again when the
 * window comes back, at most this often. */
export const HEADS_RETRY_MS = 60_000;

const NONE: ReadonlyMap<string, string> = new Map();

/** The question being asked: the rail and Accounts share it instead of asking twice. */
let asking: Promise<unknown> | null = null;

function askHeads(): Promise<unknown> {
  if (asking === null) {
    const question: Promise<unknown> = ipc.skinsMicrosoftHeads().finally(() => {
      if (asking === question) asking = null;
    });
    asking = question;
  }
  return asking;
}

/**
 * The heads of the Microsoft accounts signed in here, by profile ID: the skin Mojang
 * serves for each, cut by the core. Asked again when the accounts change, and when the
 * window comes back while one is missing (the core keeps no miss when Mojang could
 * not be reached). An account without one, or an answer that is not a PNG, gets the
 * default face.
 */
export function useMicrosoftHeads(
  accounts: readonly MicrosoftAccount[],
): ReadonlyMap<string, string> {
  const key = accounts
    .map((account) => account.profileId)
    .sort()
    .join(' ');
  const [shown, setShown] = useState<Heads | null>(null);
  const [tries, setTries] = useState(0);
  const heads = shown?.for === key ? shown.heads : NONE;
  const missing = key !== '' && key.split(' ').some((id) => !heads.has(id));

  useEffect(() => {
    if (key === '') return;
    let live = true;
    askHeads()
      .then((answer) => {
        if (!live) return;
        const ids = key.split(' ');
        const found = new Map<string, string>();
        if (typeof answer === 'object' && answer !== null && !Array.isArray(answer)) {
          for (const id of ids) {
            const head: unknown = Object.prototype.hasOwnProperty.call(answer, id)
              ? (answer as Record<string, unknown>)[id]
              : null;
            if (isPngDataUrl(head)) found.set(id, head);
          }
        }
        setShown({ for: key, heads: found });
      })
      // Only pictures: the faces shown stay.
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [key, tries]);

  useEffect(() => {
    if (!missing) return;
    const asked = Date.now();
    const onFocus = () => {
      if (Date.now() - asked >= HEADS_RETRY_MS) setTries((n) => n + 1);
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.removeEventListener('focus', onFocus);
    };
  }, [missing, key, tries]);

  return heads;
}
