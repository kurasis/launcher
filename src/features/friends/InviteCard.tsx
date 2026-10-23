import { useState } from 'react';
import { Button } from '../../components/Button';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Friends.module.css';
import { type Invite, installationsFor } from './invite';

const t = strings.friends;

/** What the launcher knows about the game for invites, and can do with them. */
export interface GameForInvites {
  /** The server the running game is on, as an invite to send; null when none. */
  invite: Invite | null;
  instances: InstanceSummary[];
  /** A game runs now: joining another server waits until it ends. */
  running: boolean;
  /** Shows a room in Worlds after joining it from a world invite. */
  openWorld: (room: string) => void;
}

/** "Minecraft 1.21.1" or "Minecraft 1.21.1 · Fabric". */
function gameName(invite: Invite): string {
  const game = strings.loaders.game({ version: invite.version });
  return invite.loader === null ? game : `${game} · ${strings.loaders.names[invite.loader]}`;
}

/**
 * An invite in the chat: the server and the game, and for the invited player
 * an installation of that version to join with. Nothing starts without the
 * player pressing Join; the core checks the address again.
 */
export function InviteCard({
  invite,
  from,
  mine,
  game,
}: {
  invite: Invite;
  from: string;
  mine: boolean;
  game: GameForInvites;
}) {
  const choices = installationsFor(invite, game.instances);
  const [chosen, setChosen] = useState(choices[0]?.id ?? '');
  const join = useCommand((requestId: string, id: string) =>
    ipc.launchGame(id, requestId, invite.server),
  );
  const target = choices.some((c) => c.id === chosen) ? chosen : (choices[0]?.id ?? '');

  return (
    <div className={styles.card}>
      <p className={styles.cardTitle}>{mine ? t.youInvited : t.invitedTo({ nick: from })}</p>
      <p className={styles.cardFacts}>
        <code className={styles.server} dir="ltr">
          {invite.server}
        </code>
        <span>{gameName(invite)}</span>
      </p>
      {!mine &&
        (game.running ? (
          <p className={styles.muted}>{t.gameRunning}</p>
        ) : choices.length === 0 ? (
          <p className={styles.muted}>{t.noInstallation({ game: gameName(invite) })}</p>
        ) : (
          <div className={styles.inline}>
            {choices.length > 1 && (
              <select
                className={forms.select}
                aria-label={t.joinWith}
                value={target}
                onChange={(event) => {
                  setChosen(event.target.value);
                }}
              >
                {choices.map((instance) => (
                  <option key={instance.id} value={instance.id}>
                    {instance.displayName}
                  </option>
                ))}
              </select>
            )}
            <Button
              variant="primary"
              size="small"
              disabled={target === ''}
              aria-disabled={join.pending || undefined}
              onClick={() => {
                if (!join.pending) void join.run(target);
              }}
            >
              {join.pending ? t.joining : t.join}
            </Button>
          </div>
        ))}
      {join.error !== null && (
        <p className={styles.error} role="alert">
          {join.error}
        </p>
      )}
    </div>
  );
}
