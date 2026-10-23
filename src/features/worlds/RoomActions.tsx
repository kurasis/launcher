import { useId, useRef, type Ref } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { useFocusBack } from '../../components/useFocusBack';
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
  startRef,
}: {
  actions: RoomActionsApi;
  room: WorldsRoom;
  local: LocalWorld | null;
  state: WorldsState;
  size?: 'small' | 'normal';
  /** Open for friends or Join: the focus goes there once the EULA is accepted. */
  startRef?: Ref<HTMLButtonElement> | undefined;
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
          ref={startRef ?? null}
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
          ref={startRef ?? null}
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
 * it is lost; Close once the hosted world has ended.
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
  const hosting = state.hosting?.room === roomId && state.hosting.ended === null;
  const ended =
    state.hosting?.room === roomId && state.hosting.ended !== null && !state.hosting.reopening;
  const guest = state.guest?.room === roomId ? state.guest : null;
  const lost = guest !== null && !guest.connected && !guest.reconnecting;
  // Play waits for the server, the connection and the mods question, and keeps the
  // focus meanwhile; the ways out wait only for each other.
  const canPlay = hosting
    ? state.hosting?.phase === 'ready'
    : guest?.connected === true && actions.consent === null;
  const playPending = actions.busy || actions.leaving || !canPlay || undefined;
  const leaving = actions.leaving || undefined;
  const playButton = (
    <Button
      ref={playRef ?? null}
      variant="primary"
      size={size}
      aria-disabled={playPending}
      aria-describedby={describedBy}
      onClick={() => {
        if (canPlay) actions.play();
      }}
    >
      {size === 'normal' && <Icon name="play" size={18} />}
      {t.play}
    </Button>
  );
  return (
    <>
      {hosting && (
        <>
          {playButton}
          <Button size={size} aria-disabled={leaving} onClick={actions.stopHosting}>
            {t.stopHosting}
          </Button>
        </>
      )}
      {ended && (
        <Button size={size} aria-disabled={leaving} onClick={actions.stopHosting}>
          {t.close}
        </Button>
      )}
      {guest !== null && !lost && playButton}
      {lost && (
        <Button variant="primary" size={size} aria-disabled={leaving} onClick={actions.joinAgain}>
          {t.guest.again}
        </Button>
      )}
      {guest !== null && (
        <Button size={size} aria-disabled={leaving} onClick={actions.disconnect}>
          {t.disconnect}
        </Button>
      )}
    </>
  );
}

/** The mods question before a guest plays, and what the last press is doing or did. */
export function RoomMessages({
  actions,
  acceptRef,
}: {
  actions: RoomActionsApi;
  /** Install the mods: it takes the focus from Play when the question comes. */
  acceptRef?: Ref<HTMLButtonElement> | undefined;
}) {
  const { consent, note, error } = actions;
  const pending = actions.busy || undefined;
  const titleId = useId();
  const newId = useId();
  return (
    <>
      {consent !== null && (
        <Notice tone="warning">
          <strong id={titleId}>{t.consent.title({ mods: consent.mods })}</strong>
          <p id={newId}>
            {t.consent.new({ count: consent.newMods, size: formatBytes(consent.bytes) })}{' '}
            {consent.names.join(', ')}
            {consent.newMods > consent.names.length &&
              ` ${t.consent.more({ count: consent.newMods - consent.names.length })}`}
          </p>
          <p>{t.consent.warning}</p>
          <div className={styles.actions}>
            <Button
              ref={acceptRef ?? null}
              variant="primary"
              size="small"
              aria-disabled={pending}
              aria-describedby={`${titleId} ${newId}`}
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
  const start = useRef<HTMLButtonElement>(null);
  // Accept the EULA goes with its answer: Open for friends takes the focus.
  useFocusBack(local?.eulaAccepted, start);
  const owner = room.role === 'owner';
  return (
    <div className={styles.actions}>
      <RoomStartButtons
        actions={actions}
        room={room}
        local={local}
        state={state}
        startRef={start}
      />
      {owner && local === null && <span className={styles.muted}>{t.keptElsewhere}</span>}
      {owner && local !== null && !local.eulaAccepted && (
        <span className={styles.muted}>{t.eulaNeeded}</span>
      )}
      <RoomMessages actions={actions} />
    </div>
  );
}
