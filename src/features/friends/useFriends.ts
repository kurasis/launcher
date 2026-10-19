import { useEffect, useState } from 'react';
import type {
  ChatRoom,
  FriendsProblem,
  FriendsSanction,
  FriendsView,
  RoomMember,
} from '../../lib/contracts';
import { ipc, onFriendsState } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';

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
