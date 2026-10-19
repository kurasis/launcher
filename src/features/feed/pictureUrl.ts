/** A picture name the feed may use (D-067): its article pictures, video and stream previews. */
const PICTURE = /^(?:img\/[0-9a-f]{64}|yt\/[A-Za-z0-9_-]{11}|tw\/[a-z0-9_]{3,25})\.jpg$/;

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
