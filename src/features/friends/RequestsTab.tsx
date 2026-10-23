import { useId, useRef } from 'react';
import { Button } from '../../components/Button';
import type { FriendAction, FriendPerson, FriendsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Friends.module.css';
import { useRowFocus } from './focus';
import { PlayerHead } from './People';
import { useAction } from './useAction';

const t = strings.friends;

/** The Requests tab: who asks (Accept, Decline, Block, Spam, Decline all) and who was asked. */
export function RequestsTab({
  view,
  heads,
}: {
  view: FriendsView;
  heads: ReadonlyMap<string, string>;
}) {
  const action = useAction();
  const incomingId = useId();
  const outgoingId = useId();
  const incomingList = useRef<HTMLUListElement>(null);
  const outgoingList = useRef<HTMLUListElement>(null);
  const incomingFocus = useRowFocus(incomingList, view.incoming);
  const outgoingFocus = useRowFocus(outgoingList, view.outgoing);
  // A press while another action runs is ignored; the button keeps the focus.
  const busy = action.pending || undefined;
  const run = (work: () => Promise<unknown>) => {
    if (!action.pending) void action.run(work);
  };
  const act = (what: FriendAction, person: FriendPerson) => {
    run(() => ipc.friendsAct(what, person.id));
  };
  if (view.incoming.length === 0 && view.outgoing.length === 0) {
    return (
      <div className={styles.tabBody}>
        <p className={styles.muted}>{t.noRequests}</p>
      </div>
    );
  }
  return (
    <div className={styles.tabBody}>
      {action.error !== null && (
        <p className={styles.error} role="alert">
          {action.error}
        </p>
      )}
      {view.incoming.length > 0 && (
        <section className={styles.group} aria-labelledby={incomingId}>
          <div className={styles.groupHead}>
            <h3 id={incomingId} className={styles.subtitle}>
              {t.incoming}
            </h3>
            {view.incoming.length > 1 && (
              <Button
                size="small"
                aria-disabled={busy}
                onClick={() => {
                  run(() => ipc.friendsDeclineAll());
                }}
              >
                {t.declineAll}
              </Button>
            )}
          </div>
          <ul className={styles.people} ref={incomingList} {...incomingFocus}>
            {view.incoming.map((person) => (
              <li key={person.id} className={styles.person}>
                <PlayerHead nick={person.nick} heads={heads} />
                <span className={styles.who}>
                  <span className={styles.nick}>{person.nick}</span>
                </span>
                <span className={styles.actions}>
                  <Button
                    size="small"
                    variant="primary"
                    aria-disabled={busy}
                    onClick={() => {
                      act('accept', person);
                    }}
                  >
                    {t.accept}
                  </Button>
                  <Button
                    size="small"
                    aria-disabled={busy}
                    onClick={() => {
                      act('decline', person);
                    }}
                  >
                    {t.decline}
                  </Button>
                  <Button
                    size="small"
                    variant="ghost"
                    aria-disabled={busy}
                    onClick={() => {
                      act('block', person);
                    }}
                  >
                    {t.block}
                  </Button>
                  <Button
                    size="small"
                    variant="ghost"
                    aria-disabled={busy}
                    onClick={() => {
                      run(() => ipc.friendsReportRequest(person.id));
                    }}
                  >
                    {t.spam}
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {view.outgoing.length > 0 && (
        <section className={styles.group} aria-labelledby={outgoingId}>
          <h3 id={outgoingId} className={styles.subtitle}>
            {t.outgoing}
          </h3>
          <ul className={styles.people} ref={outgoingList} {...outgoingFocus}>
            {view.outgoing.map((person) => (
              <li key={person.id} className={styles.person}>
                <PlayerHead nick={person.nick} heads={heads} />
                <span className={styles.who}>
                  <span className={styles.nick}>{person.nick}</span>
                </span>
                <span className={styles.actions}>
                  <Button
                    size="small"
                    variant="ghost"
                    aria-disabled={busy}
                    onClick={() => {
                      act('cancel', person);
                    }}
                  >
                    {t.cancel}
                  </Button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
