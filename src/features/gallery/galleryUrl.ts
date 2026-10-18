import type { Screenshot } from '../../lib/contracts';

/**
 * Address of a screenshot on the `gallery:` protocol that Rust serves (D-050).
 * The file's time is part of it, so a changed file is not shown from cache.
 */
export function galleryUrl(size: 'thumb' | 'full', instanceId: string, shot: Screenshot): string {
  const path = `${size}/${instanceId}/${encodeURIComponent(shot.file)}?v=${encodeURIComponent(shot.takenAt)}`;
  // Tauri serves custom protocols as http://<name>.localhost on Windows and Android.
  const agent = navigator.userAgent;
  return agent.includes('Windows') || agent.includes('Android')
    ? `http://gallery.localhost/${path}`
    : `gallery://localhost/${path}`;
}
