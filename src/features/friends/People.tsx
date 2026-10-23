import { Icon } from '../../components/Icon';
import { SkinHead } from '../../components/SkinHead';
import type { ChatRoom, FriendPerson, RoomMember } from '../../lib/contracts';
import styles from './Friends.module.css';
import { presenceText } from './useFriends';

/** A player's face by nick: the head the core cut from their skin, or the default face. */
export function PlayerHead({
  nick,
  heads,
  size = 32,
}: {
  nick: string;
  heads: ReadonlyMap<string, string>;
  size?: number;
}) {
  return <SkinHead texture={null} head={heads.get(nick) ?? null} size={size} />;
}

/** The face of the other one in a chat of two, or a group's tile. Decoration only: the
 * chat's name beside it says who. */
export function RoomAvatar({
  room,
  me,
  heads,
  size,
}: {
  room: ChatRoom;
  me: RoomMember | null;
  heads: ReadonlyMap<string, string>;
  size: number;
}) {
  if (room.kind === 'group') {
    return (
      <span
        className={styles.groupTile}
        style={{ inlineSize: size, blockSize: size }}
        aria-hidden="true"
      >
        <Icon name="users" size={Math.round(size * 0.56)} />
      </span>
    );
  }
  const other = room.members.find((m) => m.id !== me?.id);
  return other === undefined ? (
    <SkinHead texture={null} size={size} />
  ) : (
    <PlayerHead nick={other.nick} heads={heads} size={size} />
  );
}

/** A dot and "In the world Our base", "Playing …", "Online" or "Offline". */
export function Presence({ person }: { person: Pick<FriendPerson, 'online' | 'playing'> }) {
  return (
    <span className={styles.presence} data-online={person.online ? 'true' : undefined}>
      <span className={styles.dot} aria-hidden="true" />
      <span className={styles.presenceText} dir="auto">
        {presenceText(person)}
      </span>
    </span>
  );
}
