// An invite to the server the game is on (D-064): an ordinary chat message
// that reads well as text and that the launcher turns into a card with "Join".
//
//   🎮 invite play.example.com:25565 1.21.1 fabric
//
// The address and version come from another player, so they are checked here
// for the card and again by the core before anything starts.

import type { InstanceSummary, LoaderKind } from '../../lib/contracts';

export interface Invite {
  /** `host:port` */
  server: string;
  version: string;
  /** The mod loader the inviting game runs, or null for vanilla. */
  loader: LoaderKind | null;
}

const PREFIX = '🎮 invite ';
const LOADERS: readonly LoaderKind[] = ['fabric', 'quilt', 'forge', 'neoforge'];
const HOST_NAME =
  /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)*[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/i;
const IPV6 = /^\[[0-9a-f:.]{2,45}\]$/i;
const VERSION = /^[0-9A-Za-z._+-]{1,40}$/;

/** `host:port`, checked loosely here (the core checks it exactly). */
export function validServer(server: string): boolean {
  const at = server.lastIndexOf(':');
  if (at <= 0) return false;
  const host = server.slice(0, at);
  const port = Number(server.slice(at + 1));
  if (!/^\d{1,5}$/.test(server.slice(at + 1)) || port < 1 || port > 65535) return false;
  return HOST_NAME.test(host) || IPV6.test(host);
}

export function inviteBody(invite: Invite): string {
  return `${PREFIX}${invite.server} ${invite.version}${invite.loader === null ? '' : ` ${invite.loader}`}`;
}

/** The invite in a message, or null when the message is just text. */
export function parseInvite(body: string): Invite | null {
  if (!body.startsWith(PREFIX)) return null;
  const parts = body.slice(PREFIX.length).split(' ');
  if (parts.length < 2 || parts.length > 3) return null;
  const [server, version, loader] = parts as [string, string, string | undefined];
  if (!validServer(server) || !VERSION.test(version)) return null;
  if (loader !== undefined && !(LOADERS as readonly string[]).includes(loader)) return null;
  return { server, version, loader: (loader as LoaderKind | undefined) ?? null };
}

/** Installations that can join: the same Minecraft version, the same loader first. */
export function installationsFor(invite: Invite, instances: InstanceSummary[]): InstanceSummary[] {
  const same = instances.filter((i) => i.versionId === invite.version && i.health === 'ready');
  const loaderOf = (i: InstanceSummary) => i.loader?.kind ?? null;
  return [
    ...same.filter((i) => loaderOf(i) === invite.loader),
    ...same.filter((i) => loaderOf(i) !== invite.loader),
  ];
}
