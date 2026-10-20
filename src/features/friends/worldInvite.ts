// An invite to a world with friends (D-068): an ordinary chat message with
// the room's invite code, which the launcher turns into a card.
//
//   🌍 world AbCdEfGhIjKlMnOpQrStUv
//
// The code comes from another player: only its shape is checked here, the
// worlds service decides where it leads.

const PREFIX = '🌍 world ';
const CODE = /^[A-Za-z0-9_-]{22}$/;

export function worldInviteBody(code: string): string {
  return `${PREFIX}${code}`;
}

/** The invite code in a message, or null when the message is something else. */
export function parseWorldInvite(body: string): string | null {
  if (!body.startsWith(PREFIX)) return null;
  const code = body.slice(PREFIX.length);
  return CODE.test(code) ? code : null;
}

/** Presence of a game in a room's world, as the core reports it. */
const WORLD_PRESENCE = '🌍 ';

/** The world's name when a friend plays in one, else null. */
export function worldOfPresence(playing: string): string | null {
  if (!playing.startsWith(WORLD_PRESENCE)) return null;
  const name = playing.slice(WORLD_PRESENCE.length).trim();
  return name === '' ? null : name;
}
