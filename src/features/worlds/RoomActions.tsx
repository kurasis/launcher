import type { Ref } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useRoomActions, type RoomActionsApi, type RoomTarget } from './useRoomActions';
import styles from './Worlds.module.css';

const t = strings.worlds;

/** What can be pressed for a room while this computer neither hosts nor joins it. */
export function RoomStartButtons({
  actions,
  room,
  local,
  state,
  size,
}: {
  actions: RoomActionsApi;
  room: WorldsRoom;
  local: LocalWorld | null;
  state: WorldsState;
  size?: 'small' | 'normal';
}) {
  const inRoom = state.preparing !== null || state.hosting !== null || state.guest !== null;
  const owner = room.role === 'owner';
  const pending = actions.busy || undefined;
  // In a row the buttons stay quiet: the room being played has the one accent.
  const variant = size === 'small' ? 'secondary' : 'primary';
  const sized = size ?? 'normal';
  return (
    <>
      {owner && local !== null && !local.eulaAccepted && (
        <>
          <Button
            variant={variant}
            size={sized}
            aria-disabled={pending}
            onClick={actions.acceptEula}
          >
            {t.acceptEula}
          </Button>
          <Button
            size={sized}
            onClick={() => {
              void ipc.openExternal('minecraft_eula').catch(() => undefined);
            }}
          >
            {t.readEula}
          </Button>
        </>
      )}
      {owner && local?.eulaAccepted === true && !inRoom && (
        <Button
          variant={variant}
          size={sized}
          disabled={state.maintenance}
          aria-disabled={pending}
          onClick={actions.host}
        >
          {t.host}
        </Button>
      )}
      {!owner && room.hosting && !inRoom && (
        <Button
          variant={variant}
          size={sized}
          disabled={state.maintenance}
          aria-disabled={pending}
          onClick={actions.join}
        >
          {t.join}
        </Button>
      )}
    </>
  );
}

/**
 * Play and the way out for the room this computer hosts or is a guest in: Close the
 * world, or Leave the world in every state of the connection, with Join again once
 * it is lost.
 */
export function RoomPlayButtons({
  actions,
  roomId,
  state,
  playRef,
  describedBy,
  size = 'normal',
}: {
  actions: RoomActionsApi;
  roomId: string;
  state: WorldsState;
  size?: 'small' | 'normal';
  playRef?: Ref<HTMLButtonElement> | undefined;
  /** What Play plays, for assistive technology: the room's line. */
  describedBy?: string | undefined;
}) {
  const pending = actions.busy || undefined;
  const hosting = state.hosting?.room === roomId && state.hosting.ended === null;
  const guest = state.guest?.room === roomId ? state.guest : null;
  const lost = guest !== null && !guest.connected && !guest.reconnecting;
  return (
    <>
      {hosting && (
        <>
          <Button
            ref={playRef ?? null}
            variant="primary"
            size={size}
            disabled={state.hosting?.phase !== 'ready'}
            aria-disabled={pending}
            aria-describedby={describedBy}
            onClick={() => {
              actions.play();
            }}
          >
            {t.play}
          </Button>
          <Button size={size} aria-disabled={pending} onClick={actions.stopHosting}>
            {t.stopHosting}
          </Button>
        </>
      )}
      {guest !== null && !lost && (
        <Button
          ref={playRef ?? null}
          variant="primary"
          size={size}
          disabled={!guest.connected || actions.consent !== null}
          aria-disabled={pending}
          aria-describedby={describedBy}
          onClick={() => {
            actions.play();
          }}
        >
          {t.play}
        </Button>
      )}
      {lost && (
        <Button variant="primary" size={size} aria-disabled={pending} onClick={actions.joinAgain}>
          {t.guest.again}
        </Button>
      )}
      {guest !== null && (
        <Button size={size} aria-disabled={pending} onClick={actions.disconnect}>
          {t.disconnect}
        </Button>
      )}
    </>
  );
}

/** The mods question before a guest plays, and what the last press is doing or did. */
export function RoomMessages({ actions }: { actions: RoomActionsApi }) {
  const { consent, note, error } = actions;
  const pending = actions.busy || undefined;
  return (
    <>
      {consent !== null && (
        <Notice tone="warning">
          <strong>{t.consent.title({ mods: consent.mods })}</strong>
          <p>
            {t.consent.new({ count: consent.newMods, size: formatBytes(consent.bytes) })}{' '}
            {consent.names.join(', ')}
            {consent.newMods > consent.names.length &&
              ` ${t.consent.more({ count: consent.newMods - consent.names.length })}`}
          </p>
          <p>{t.consent.warning}</p>
          <div className={styles.actions}>
            <Button
              variant="primary"
              size="small"
              aria-disabled={pending}
              onClick={() => {
                actions.play(consent.sha256);
              }}
            >
              {t.consent.accept}
            </Button>
            <Button size="small" aria-disabled={pending} onClick={actions.decline}>
              {t.consent.decline}
            </Button>
          </div>
        </Notice>
      )}
      {note !== null && <Notice tone="info">{note}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </>
  );
}

/** A room page's actions: start it, and what the room's state says about this computer. */
export function RoomActions({
  room,
  local,
  state,
  onLocal,
}: Omit<RoomTarget, 'roomId'> & {
  room: WorldsRoom;
}) {
  const actions = useRoomActions({ roomId: room.id, room, local, onLocal });
  const owner = room.role === 'owner';
  return (
    <div className={styles.actions}>
      <RoomStartButtons actions={actions} room={room} local={local} state={state} />
      {owner && local === null && <span className={styles.muted}>{t.keptElsewhere}</span>}
      {owner && local !== null && !local.eulaAccepted && (
        <span className={styles.muted}>{t.eulaNeeded}</span>
      )}
      <RoomMessages actions={actions} />
    </div>
  );
}
