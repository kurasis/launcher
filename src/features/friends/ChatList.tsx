import type { ChatRoom, FriendsView } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { listTime } from './chatTime';
import styles from './Friends.module.css';
import { RoomAvatar } from './People';
import { previewText, roomTitle } from './useFriends';

const t = strings.friends;

/**
 * The chats beside the conversation: each with a face or a group tile, its name, when
 * the last message came and what it said; unread chats first, under their own label.
 */
export function ChatList({
  view,
  heads,
  chosen,
  onOpen,
}: {
  view: FriendsView;
  heads: ReadonlyMap<string, string>;
  /** The chat open in the conversation pane. */
  chosen: string | null;
  onOpen: (room: string) => void;
}) {
  if (view.rooms.length === 0) return <p className={styles.muted}>{t.noChats}</p>;
  const unread = view.rooms.filter((r) => r.unread > 0);
  const read = view.rooms.filter((r) => r.unread === 0);
  const list = (rooms: ChatRoom[]) => (
    <ul className={styles.rooms}>
      {rooms.map((room) => {
        const preview = previewText(room, view.me);
        return (
          <li key={room.id}>
            <button
              type="button"
              className={styles.roomRow}
              aria-current={room.id === chosen ? 'true' : undefined}
              data-unread={room.unread > 0 ? 'true' : undefined}
              onClick={() => {
                onOpen(room.id);
              }}
            >
              <RoomAvatar room={room} me={view.me} heads={heads} size={32} />
              <span className={styles.roomText}>
                <span className={styles.roomLine}>
                  <span className={styles.roomName} dir="auto">
                    {roomTitle(room, view.me)}
                  </span>
                  {room.last !== null && (
                    <time
                      className={styles.roomTime}
                      dateTime={new Date(room.last.created * 1000).toISOString()}
                    >
                      {listTime(room.last.created)}
                    </time>
                  )}
                </span>
                <span className={styles.roomLine}>
                  <span className={styles.preview} dir="auto">
                    {preview}
                  </span>
                  {room.unread > 0 && (
                    <span className={styles.count} aria-label={t.unread({ count: room.unread })}>
                      {room.unread > 99 ? '99+' : room.unread}
                    </span>
                  )}
                </span>
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
  if (unread.length === 0) return list(read);
  return (
    <>
      <h3 className={styles.listLabel}>{t.unreadSection}</h3>
      {list(unread)}
      {read.length > 0 && (
        <>
          <h3 className={styles.listLabel}>{t.readSection}</h3>
          {list(read)}
        </>
      )}
    </>
  );
}
