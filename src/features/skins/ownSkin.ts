/** Who shows the player's own skin (the rail's head) hears when it is uploaded or removed. */
const listeners = new Set<() => void>();

export function ownSkinChanged(): void {
  for (const listener of listeners) listener();
}

export function onOwnSkinChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
