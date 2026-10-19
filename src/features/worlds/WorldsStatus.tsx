import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { WorldsRoom, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { failedText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  state: WorldsState;
  rooms: WorldsRoom[];
}

/** Getting ready, hosting or being a guest: what this computer does now. */
export function WorldsStatus({ state, rooms }: Props) {
  const t = strings.worlds;
  const nameOf = (id: string) => rooms.find((r) => r.id === id)?.name ?? '';
  const { preparing, hosting, guest, failed } = state;

  return (
    <>
      {preparing !== null && (
        <div className={styles.status} role="status">
          <p className={styles.statusTitle}>{t.stages[preparing.stage]}</p>
          {preparing.totalBytes > 0 && (
            <>
              <progress max={preparing.totalBytes} value={preparing.downloadedBytes} />
              <p className={styles.muted}>
                {t.progress({
                  done: formatBytes(preparing.downloadedBytes),
                  total: formatBytes(preparing.totalBytes),
                })}
              </p>
            </>
          )}
          {preparing.stage === 'server' && (
            <div className={styles.actions}>
              <Button
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
        <div className={styles.status}>
          <p className={styles.statusTitle}>
            {t.hosting.title({ room: nameOf(hosting.room) })} · {t.hosting.phases[hosting.phase]}
          </p>
          {hosting.ended === null ? (
            <>
              <p>
                {t.hosting.players({
                  count: hosting.players.length,
                  names: hosting.players.join(', '),
                })}
              </p>
              <p className={styles.muted}>{t.hosting.hostPlays}</p>
            </>
          ) : (
            <>
              <Notice tone={hosting.ended === 'stopped' ? 'info' : 'warning'}>
                {t.hosting.ended[hosting.ended]}
              </Notice>
              {hosting.clean === false && <Notice tone="warning">{t.hosting.unclean}</Notice>}
              <div className={styles.actions}>
                <Button
                  onClick={() => {
                    void ipc.worldsStopHosting().catch(() => undefined);
                  }}
                >
                  {t.close}
                </Button>
              </div>
            </>
          )}
        </div>
      )}
      {guest !== null && (
        <div className={styles.status}>
          <p className={styles.statusTitle}>{t.guest.title({ room: nameOf(guest.room) })}</p>
          <p>{guest.connected ? t.guest.connected : t.guest.lost}</p>
          {guest.refused !== null && <Notice tone="warning">{t.refusals[guest.refused]}</Notice>}
        </div>
      )}
      {failed !== null && preparing === null && <Notice tone="danger">{failedText(failed)}</Notice>}
    </>
  );
}
