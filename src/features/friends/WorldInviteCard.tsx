import { useState } from 'react';
import { Button } from '../../components/Button';
import type { WorldsPreview } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from '../worlds/useWorlds';
import styles from './Friends.module.css';

const t = strings.friends;

/**
 * An invite to a world in the chat. The invited player looks first (nothing
 * changes), then joins the room and Worlds opens on it.
 */
export function WorldInviteCard({
  code,
  from,
  mine,
  onJoined,
}: {
  code: string;
  from: string;
  mine: boolean;
  onJoined: (room: string) => void;
}) {
  const [preview, setPreview] = useState<WorldsPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    work()
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <div className={styles.invite}>
      <p className={styles.inviteTitle}>
        {mine ? t.youInvitedWorld : t.invitedToWorld({ nick: from })}
      </p>
      {preview !== null && (
        <p className={styles.muted} role="status">
          {strings.worlds.code.preview({
            name: preview.name,
            owner: preview.ownerNick,
            version: preview.minecraft,
            members: preview.members,
          })}
        </p>
      )}
      {!mine && (
        <div className={styles.inline}>
          {preview === null ? (
            <Button
              disabled={busy}
              onClick={() => {
                run(() => ipc.worldsPreview(code).then(setPreview));
              }}
            >
              {strings.worlds.code.look}
            </Button>
          ) : (
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => {
                run(() =>
                  ipc.worldsRedeem(code).then((room) => {
                    onJoined(room.id);
                  }),
                );
              }}
            >
              {strings.worlds.code.accept}
            </Button>
          )}
        </div>
      )}
      {error !== null && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
