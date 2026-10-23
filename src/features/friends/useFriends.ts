import { useEffect, useState } from 'react';
import type {
  ChatRoom,
  FriendPerson,
  FriendsProblem,
  FriendsSanction,
  FriendsView,
  RoomMember,
} from '../../lib/contracts';
import { ipc, onFriendsState } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { parseInvite } from './invite';
import { parseWorldInvite, worldOfPresence } from './worldInvite';

/** Friends and chats as the launcher's core keeps them; null until read. */
export function useFriends(): FriendsView | null {
  const [view, setView] = useState<FriendsView | null>(null);
  useEffect(() => {
    let live = true;
    const stop = onFriendsState((next) => {
      if (live) setView(next);
    });
    void ipc
      .friendsState()
      .then((first) => {
        // An event that came first is newer.
        if (live) setView((current) => current ?? first);
      })
      .catch(() => undefined);
    return () => {
      live = false;
      stop();
    };
  }, []);
  return view;
}

/** What a direct chat or group is called in the list and its header. */
export function roomTitle(room: ChatRoom, me: RoomMember | null): string {
  if (room.kind === 'group') {
    return room.name.length > 0 ? room.name : room.members.map((m) => m.nick).join(', ');
  }
  return room.members.find((m) => m.id !== me?.id)?.nick ?? room.name;
}

/**
 * The last message of a chat as its row shows it, as plain text: an invite in words,
 * "You: …" for mine, "Max: …" from someone in a group, the text alone from the other
 * one of two.
 */
export function previewText(room: ChatRoom, me: RoomMember | null): string | null {
  const t = strings.friends;
  const last = room.last;
  if (last === null) return null;
  const body =
    parseInvite(last.body) !== null
      ? t.previewServerInvite
      : parseWorldInvite(last.body) !== null
        ? t.previewWorldInvite
        : last.body;
  if (me !== null && last.sender === me.id) return `${t.you}: ${body}`;
  if (room.kind === 'direct') return body;
  return `${room.members.find((m) => m.id === last.sender)?.nick ?? '—'}: ${body}`;
}

/** "Playing Minecraft 1.21.1", "In the world Our base", "Online" or "Offline". */
export function presenceText(person: Pick<FriendPerson, 'online' | 'playing'>): string {
  const t = strings.friends;
  if (!person.online) return t.away;
  if (person.playing === null) return t.online;
  const world = worldOfPresence(person.playing);
  return world === null ? t.playing({ game: person.playing }) : t.inWorld({ name: world });
}

export function problemText(problem: FriendsProblem): string {
  return strings.friends.problems[problem];
}

/** What a mute or ban means for the player, with when it ends. */
export function sanctionText(sanction: FriendsSanction): string {
  const t = strings.friends;
  if (sanction.until === 0) {
    return sanction.kind === 'mute' ? t.mutedForGood : t.bannedForGood;
  }
  const until = new Date(sanction.until * 1000).toLocaleString(numberLocale());
  return sanction.kind === 'mute' ? t.muted({ until }) : t.banned({ until });
}
