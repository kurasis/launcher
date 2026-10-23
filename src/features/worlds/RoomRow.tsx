import { useId } from 'react';
import { Button } from '../../components/Button';
import { Tag } from '../../components/Tag';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { isolate } from '../mods/helpers';
import { RoomMessages, RoomStartButtons } from './RoomActions';
import { useRoomActions } from './useRoomActions';
import { roomMods, roomOpen } from './useWorlds';
import styles from './Worlds.module.css';

const t = strings.worlds;

interface Props {
  room: WorldsRoom;
  /** The room's world when it is kept on this computer. */
  local: LocalWorld | null;
  state: WorldsState;
  /** This computer gets ready, hosts or is a guest in this room: the band has its
   * buttons, the row only Open. */
  active: boolean;
  onLocal: (world: LocalWorld) => void;
  onOpen: () => void;
}

/** A room in the list: its name and facts, what can start it here, and Open. */
export function RoomRow({ room, local, state, active, onLocal, onOpen }: Props) {
  const actions = useRoomActions({ roomId: room.id, room, local, onLocal });
  const nameId = useId();
  const now = roomOpen(room, state);
  const mods = roomMods(room, local);
  const owner = room.role === 'owner';
  return (
    <li className={styles.roomRow}>
      <div className={styles.rowLine}>
        <span id={nameId} className={styles.rowName} dir="auto">
          {room.name}
        </span>
        <Tag tone={now.open ? 'success' : 'outline'}>
          {now.open ? t.openNow({ online: now.online, max: room.maxPlayers }) : t.closedNow}
        </Tag>
        {owner && <Tag>{local === null ? t.keptElsewhere : t.keptHere}</Tag>}
        {mods !== null && <Tag>{t.modsCount({ count: mods })}</Tag>}
      </div>
      <span className={styles.rowMeta}>
        {t.roomMeta({
          owner: isolate(room.ownerNick),
          version: room.minecraft,
          members: room.members,
        })}
      </span>
      <div className={styles.rowActions}>
        {!active && (
          <RoomStartButtons
            actions={actions}
            room={room}
            local={local}
            state={state}
            size="small"
          />
        )}
        {/* "Open", with the room's name read after it. */}
        <Button size="small" aria-describedby={nameId} onClick={onOpen}>
          {t.open}
        </Button>
      </div>
      {!active && owner && local !== null && !local.eulaAccepted && (
        <p className={styles.rowNote}>{t.eulaNeeded}</p>
      )}
      {!active && (actions.note !== null || actions.error !== null) && (
        <div className={styles.rowWide}>
          <RoomMessages actions={actions} />
        </div>
      )}
    </li>
  );
}
