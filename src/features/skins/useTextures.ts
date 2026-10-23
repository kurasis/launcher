import { useEffect, useRef, useState } from 'react';
import { ipc } from '../../lib/ipc';
import { isPngDataUrl } from './textures';

/** Textures asked of the core at once. */
const AT_ONCE = 3;

const isHash = (text: string) => /^[0-9a-f]{64}$/.test(text);

/**
 * The PNGs of these textures as `data:` URLs, by hash, fetched by the core (which
 * checks each against its hash) a few at a time, in the order given: the tab shown
 * first. '' for one that could not be fetched (asked again when the list changes);
 * none yet for one on its way. A texture the list no longer has is let go.
 */
export function useTextures(hashes: readonly string[]): ReadonlyMap<string, string> {
  const [urls, setUrls] = useState<ReadonlyMap<string, string>>(() => new Map());
  const asked = useRef(new Set<string>());
  const wanted = useRef(new Set<string>());
  const mounted = useRef(true);
  const key = [...new Set(hashes.filter(isHash))].join(' ');

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    const list = key === '' ? [] : key.split(' ');
    wanted.current = new Set(list);
    for (const hash of [...asked.current])
      if (!wanted.current.has(hash)) asked.current.delete(hash);
    let live = true;
    const queue = list.filter((hash) => !asked.current.has(hash));
    const next = async (): Promise<void> => {
      const hash = queue.shift();
      if (hash === undefined || !live) return;
      asked.current.add(hash);
      let url = '';
      try {
        const answer: unknown = await ipc.skinsTexture(hash);
        if (isPngDataUrl(answer)) url = answer;
      } catch {
        // The card says the picture did not come.
      }
      if (url === '') asked.current.delete(hash);
      if (mounted.current && wanted.current.has(hash)) {
        setUrls((old) => {
          const kept = new Map([...old].filter(([h]) => wanted.current.has(h)));
          return kept.set(hash, url);
        });
      }
      return next();
    };
    for (let i = 0; i < AT_ONCE; i++) void next();
    return () => {
      live = false;
    };
  }, [key]);

  return urls;
}
