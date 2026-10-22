/**
 * A picture name the feed may use (D-067): its article pictures, video previews
 * (upright ones of Shorts too), stream previews and channel pictures.
 */
const PICTURE =
  /^(?:img\/[0-9a-f]{64}|y[ts]\/[A-Za-z0-9_-]{11}|t[wa]\/[a-z0-9_]{3,25}|ya\/UC[A-Za-z0-9_-]{22})\.jpg$/;

/**
 * Address of a feed picture on the `feed:` protocol that Rust serves; null for
 * anything that is not one of the feed's picture names.
 */
export function pictureUrl(path: string | null): string | null {
  if (path === null || !PICTURE.test(path)) return null;
  // Tauri serves custom protocols as http://<name>.localhost on Windows and Android.
  const agent = navigator.userAgent;
  return agent.includes('Windows') || agent.includes('Android')
    ? `http://feed.localhost/${path}`
    : `feed://localhost/${path}`;
}
