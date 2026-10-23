import { useId, useRef, type RefObject } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { ProgressBar } from '../../components/ProgressBar';
import { Dot, type DotTone } from '../../components/StateDot';
import { useFocusBack } from '../../components/useFocusBack';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { isolate } from '../mods/helpers';
import { RoomMessages, RoomPlayButtons } from './RoomActions';
import { useRoomActions } from './useRoomActions';
import { activeRoom, failedText } from './useWorlds';
import styles from './Worlds.module.css';

const t = strings.worlds;

/** The band's tone: the state at a glance, beside the words that say it. */
function bandTone(state: WorldsState): DotTone {
  const { preparing, hosting, guest } = state;
  const lost = guest !== null && !guest.connected && !guest.reconnecting;
  if (hosting !== null) {
    if (hosting.reopening || (hosting.ended !== null && hosting.ended !== 'stopped')) {
      return 'warning';
    }
    if (hosting.ended === null && hosting.phase === 'ready') return 'success';
    if (hosting.ended === null && hosting.phase === 'starting') return 'busy';
  }
  if (lost) return 'warning';
  if (guest?.connected === true) return 'success';
  if (preparing !== null || guest?.reconnecting === true) return 'busy';
  return 'neutral';
}

const BAND: Record<DotTone, string | undefined> = {
  success: styles.bandSuccess,
  busy: undefined,
  warning: styles.bandWarning,
  danger: styles.bandWarning,
  neutral: undefined,
};

interface Props {
  state: WorldsState;
  rooms: WorldsRoom[];
  local: LocalWorld[];
  onLocal: (world: LocalWorld) => void;
  /** The band, to take the focus when the button that had it went with a state. */
  bandRef: RefObject<HTMLElement | null>;
}

/**
 * Getting ready, hosting or being a guest: what this computer does now, as one band
 * over the rooms (concept A), with Play and the way out of that room. Its tone follows
 * the state; the words say it.
 */
export function WorldsStatus({ state, rooms, local, onLocal, bandRef }: Props) {
  const { preparing, hosting, guest, failed } = state;
  const roomId = activeRoom(state);
  const room = rooms.find((r) => r.id === roomId) ?? null;
  const world = local.find((w) => w.id === room?.world) ?? null;
  const actions = useRoomActions({ roomId: roomId ?? '', room, local: world, onLocal });
  const titleId = useId();
  const play = useRef<HTMLButtonElement>(null);
  // A button that went with a state (Close the world, Not now) hands the focus on.
  useFocusBack(
    [
      preparing?.stage,
      hosting?.phase,
      hosting?.ended,
      hosting?.reopening,
      guest?.connected,
      guest?.reconnecting,
      actions.consent === null,
    ].join(':'),
    play,
    bandRef,
  );

  const failedNotice = failed !== null && preparing === null && (
    <Notice tone="danger">{failedText(failed)}</Notice>
  );
  if (roomId === null) return failedNotice;

  const name = room === null ? null : isolate(room.name);
  const tone = bandTone(state);

  return (
    <>
      <section
        ref={bandRef}
        className={[styles.band, BAND[tone]].filter(Boolean).join(' ')}
        aria-label={t.state.label}
        tabIndex={-1}
      >
        {preparing !== null && (
          <div className={styles.bandRow}>
            <Dot tone="busy" />
            <div className={styles.bandText}>
              {/* The stage is read out when it changes; the bytes are not, at each step. */}
              <p role="status" className={styles.bandTitle}>
                {t.stages[preparing.stage]}
              </p>
              {preparing.totalBytes > 0 && (
                <div className={styles.bandProgress}>
                  <ProgressBar
                    value={preparing.downloadedBytes}
                    max={preparing.totalBytes}
                    label={t.stages[preparing.stage]}
                  />
                  <p className={styles.bandHint}>
                    {t.progress({
                      done: formatBytes(preparing.downloadedBytes),
                      total: formatBytes(preparing.totalBytes),
                    })}
                  </p>
                </div>
              )}
            </div>
            {preparing.stage === 'server' && (
              <div className={styles.bandActions}>
                <Button
                  size="small"
                  onClick={() => {
                    void ipc.worldsCancel().catch(() => undefined);
                  }}
                >
                  {t.cancel}
                </Button>
              </div>
            )}
          </div>
        )}
        {hosting !== null && (
          <div className={styles.bandRow}>
            <Dot tone={tone} />
            <div className={styles.bandText}>
              <p id={titleId} className={styles.bandTitle}>
                {name === null ? t.hosting.titleUnknown : t.hosting.title({ room: name })}
                {hosting.phase !== 'ready' && ` · ${t.hosting.phases[hosting.phase]}`}
              </p>
              {!hosting.reopening && hosting.ended === null && (
                <>
                  <p className={styles.bandHint}>
                    {t.hosting.players({
                      count: hosting.players.length,
                      names: hosting.players.join(', '),
                    })}
                  </p>
                  <p className={styles.bandHint}>{t.hosting.hostPlays}</p>
                </>
              )}
            </div>
            <div className={styles.bandActions}>
              <RoomPlayButtons
                actions={actions}
                roomId={roomId}
                state={state}
                playRef={play}
                describedBy={titleId}
                size="small"
              />
              {hosting.ended !== null && !hosting.reopening && (
                <Button
                  size="small"
                  aria-disabled={actions.busy || undefined}
                  onClick={actions.stopHosting}
                >
                  {t.close}
                </Button>
              )}
            </div>
            {hosting.reopening ? (
              <div className={styles.bandWide}>
                <Notice tone="warning">{t.hosting.reopening}</Notice>
              </div>
            ) : (
              hosting.ended !== null && (
                <div className={styles.bandWide}>
                  <Notice tone={hosting.ended === 'stopped' ? 'info' : 'warning'}>
                    {t.hosting.ended[hosting.ended]}
                  </Notice>
                  {hosting.clean === false && <Notice tone="warning">{t.hosting.unclean}</Notice>}
                </div>
              )
            )}
          </div>
        )}
        {guest !== null && (
          <div className={styles.bandRow}>
            <Dot tone={tone} />
            <div className={styles.bandText}>
              <p id={hosting === null ? titleId : undefined} className={styles.bandTitle}>
                {name === null ? t.guest.titleUnknown : t.guest.title({ room: name })}
              </p>
              <p role="status" className={styles.bandHint}>
                {guest.connected
                  ? t.guest.connected
                  : guest.reconnecting
                    ? t.guest.reconnecting
                    : t.guest.lost}
              </p>
              {guest.connected && guest.path !== null && (
                <p className={styles.bandHint}>{t.guest.path[guest.path]}</p>
              )}
            </div>
            <div className={styles.bandActions}>
              {hosting === null && (
                <RoomPlayButtons
                  actions={actions}
                  roomId={roomId}
                  state={state}
                  playRef={play}
                  describedBy={titleId}
                  size="small"
                />
              )}
            </div>
            {guest.refused !== null && (
              <div className={styles.bandWide}>
                <Notice tone="warning">{t.refusals[guest.refused]}</Notice>
              </div>
            )}
          </div>
        )}
        {(actions.consent !== null || actions.note !== null || actions.error !== null) && (
          <div className={styles.bandWide}>
            <RoomMessages actions={actions} />
          </div>
        )}
      </section>
      {failedNotice}
    </>
  );
}
