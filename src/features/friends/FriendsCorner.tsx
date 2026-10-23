import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type { AppSnapshot, FriendsView, SettingsView } from '../../lib/contracts';
import { onFriendsMessage } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { usePlayerHeads } from '../skins/usePlayerHeads';
import { ChatList } from './ChatList';
import { ChatView } from './ChatView';
import { ConfirmEmail } from './ConfirmEmail';
import styles from './Friends.module.css';
import { FriendsTab } from './FriendsTab';
import type { GameForInvites } from './InviteCard';
import { NewGroup } from './NewGroup';
import { RequestsTab } from './RequestsTab';
import { playChime, unlockSoundOnFirstInput } from './sound';
import { Spinner } from './Spinner';
import { problemText, sanctionText, useFriends } from './useFriends';

const t = strings.friends;
type Tab = 'chats' | 'friends' | 'requests';
const TABS: readonly Tab[] = ['chats', 'friends', 'requests'];

/** The running game's server as an invite, and the installations to join with. */
function gameForInvites(snapshot: AppSnapshot, openWorld: (room: string) => void): GameForInvites {
  const run = snapshot.currentRun;
  const running =
    run !== null &&
    run.kind === 'game' &&
    !['exited', 'failed', 'cancelled', 'stopped'].includes(run.state);
  const instance =
    run?.instanceId == null ? undefined : snapshot.instances.find((i) => i.id === run.instanceId);
  return {
    invite:
      running && run.server !== null && instance !== undefined
        ? { server: run.server, version: instance.versionId, loader: instance.loader?.kind ?? null }
        : null,
    instances: snapshot.instances,
    running,
    openWorld,
  };
}

/** Where the sheet was: it opens again on the same tab and chat. */
interface Place {
  tab: Tab;
  room: string | null;
  /** A new group is being made in the conversation pane. */
  grouping: boolean;
}

/**
 * Friends and chats of the launcher account (D-064): a row of concept A's rail with a
 * count of unread messages and requests, and the whole friends system in a side sheet
 * at the window's inline end (D-072).
 */
export function FriendsCorner({
  snapshot,
  onOpenAccounts,
  onOpenWorld,
}: {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
  /** Shows a room in Worlds, such as one joined from a chat invite. */
  onOpenWorld: (room: string) => void;
}) {
  const settings = snapshot.settings;
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<Place>({ tab: 'chats', room: null, grouping: false });
  const buttonRef = useRef<HTMLButtonElement>(null);
  const game = gameForInvites(snapshot, (room) => {
    setOpen(false);
    onOpenWorld(room);
  });
  const view = useFriends();
  // The chat on screen makes no sound for its own messages.
  useChime(view, settings, open && place.tab === 'chats' ? place.room : null);
  if (view === null) return null;
  const waiting = view.unread + view.incoming.length;
  // Closing puts focus back on the rail's row.
  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };
  return (
    <div
      className={styles.railSlot}
      onKeyDown={(event) => {
        // An inner control that took the key (a menu, a question) keeps the sheet open.
        if (event.key === 'Escape' && open && !event.defaultPrevented) close();
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className={styles.railButton}
        aria-label={view.unread > 0 ? `${t.open}, ${t.unread({ count: view.unread })}` : t.open}
        aria-expanded={open}
        title={t.open}
        onClick={() => {
          if (open) close();
          else setOpen(true);
        }}
      >
        <Icon name="friends" className={styles.railIcon} />
        <span className={styles.railLabel} aria-hidden="true">
          {t.title}
        </span>
        {view.signedIn && waiting > 0 && (
          <span className={styles.badge}>{waiting > 99 ? '99+' : waiting}</span>
        )}
      </button>
      {/* After its row, so Tab goes from one into the other. */}
      {open && (
        <>
          <FriendsSheet
            view={view}
            settings={settings}
            game={game}
            place={place}
            onPlace={setPlace}
            onOpenAccounts={() => {
              setOpen(false);
              onOpenAccounts();
            }}
            onClose={close}
          />
          <div className={styles.scrim} aria-hidden="true" onClick={close} />
        </>
      )}
    </div>
  );
}

/**
 * The chime for a message from someone else or a new friend request. While the
 * launcher is not in front, the system notification (made by the core) speaks
 * instead, unless the player turned those off. A chat open on screen makes no
 * sound for its own messages.
 */
function useChime(view: FriendsView | null, settings: SettingsView, openRoom: string | null) {
  const me = view?.me?.id ?? null;
  const latest = useRef({ me, settings, openRoom });
  useEffect(() => {
    latest.current = { me, settings, openRoom };
  }, [me, settings, openRoom]);
  const requests = useRef<Set<string> | null>(null);

  useEffect(() => unlockSoundOnFirstInput(), []);

  const wanted = () => {
    const { settings } = latest.current;
    return settings.chatSound && (document.hasFocus() || !settings.chatNotifications);
  };

  useEffect(
    () =>
      onFriendsMessage((message) => {
        const { me, openRoom } = latest.current;
        if (message.sender === me) return;
        if (message.room === openRoom && document.hasFocus()) return;
        if (wanted()) playChime();
      }),
    [],
  );

  // A request that was not there before: the first state only remembers.
  const incoming = view?.incoming;
  useEffect(() => {
    if (incoming === undefined) return;
    const ids = new Set(incoming.map((p) => p.id));
    const before = requests.current;
    requests.current = ids;
    if (before !== null && [...ids].some((id) => !before.has(id)) && wanted()) playChime();
  }, [incoming]);
}

/** Every player the sheet may show a face of, once. */
function nicksOf(view: FriendsView): string[] {
  const people = [...view.friends, ...view.incoming, ...view.outgoing, ...view.blocked];
  const members = view.rooms.flatMap((room) => room.members);
  return [...people, ...members]
    .filter((person) => person.id !== view.me?.id)
    .map((person) => person.nick);
}

function FriendsSheet({
  view,
  settings,
  game,
  place,
  onPlace,
  onOpenAccounts,
  onClose,
}: {
  view: FriendsView;
  settings: SettingsView;
  game: GameForInvites;
  place: Place;
  onPlace: (place: Place) => void;
  onOpenAccounts: () => void;
  onClose: () => void;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  const headingId = useId();
  const tabsId = useId();
  const heads = usePlayerHeads(nicksOf(view));
  // Opening the sheet moves the focus into it.
  useEffect(() => {
    heading.current?.focus();
  }, []);
  const unconfirmed = view.problem === 'email_not_confirmed';
  const ready = view.signedIn && view.problem === null && view.me !== null;
  const { tab } = place;
  const room = place.room === null ? undefined : view.rooms.find((r) => r.id === place.room);
  const show = (next: Partial<Place>) => {
    onPlace({ ...place, ...next });
  };
  const openRoom = (id: string) => {
    onPlace({ tab: 'chats', room: id, grouping: false });
  };
  const fewFriends = view.friends.length < 2;

  let body;
  if (unconfirmed) {
    body = <ConfirmEmail onOpenAccounts={onOpenAccounts} />;
  } else if (!view.signedIn) {
    body = (
      <div className={styles.state}>
        <p>{t.signedOut}</p>
        <Button variant="primary" onClick={onOpenAccounts}>
          {t.goToAccounts}
        </Button>
      </div>
    );
  } else {
    const label = (item: Tab) => {
      const count = item === 'chats' ? view.unread : item === 'requests' ? view.incoming.length : 0;
      return count > 0 ? t.tabCount({ label: t.tabs[item], count }) : t.tabs[item];
    };
    body = (
      <>
        <div
          role="tablist"
          aria-label={t.title}
          className={styles.tabs}
          onKeyDown={(event) => {
            const keys: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1 };
            let step = keys[event.key];
            const at = TABS.indexOf(tab);
            let next: Tab | undefined;
            if (event.key === 'Home') next = TABS[0];
            else if (event.key === 'End') next = TABS[TABS.length - 1];
            else if (step !== undefined) {
              if (getComputedStyle(event.currentTarget).direction === 'rtl') step = -step;
              next = TABS[(at + step + TABS.length) % TABS.length];
            }
            if (next === undefined) return;
            event.preventDefault();
            show({ tab: next });
            event.currentTarget.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
          }}
        >
          {TABS.map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              id={`${tabsId}-${item}`}
              data-tab={item}
              aria-selected={tab === item}
              aria-controls={`${tabsId}-panel`}
              tabIndex={tab === item ? 0 : -1}
              className={styles.tab}
              onClick={() => {
                show({ tab: item });
              }}
            >
              {label(item)}
            </button>
          ))}
        </div>
        <div
          role="tabpanel"
          id={`${tabsId}-panel`}
          aria-labelledby={`${tabsId}-${tab}`}
          className={styles.tabPanel}
        >
          {tab === 'chats' && (
            <div className={styles.chats}>
              <div className={styles.chatList}>
                <ChatList view={view} heads={heads} chosen={room?.id ?? null} onOpen={openRoom} />
              </div>
              <div className={styles.pane}>
                {place.grouping ? (
                  <NewGroup
                    view={view}
                    heads={heads}
                    onDone={(created) => {
                      onPlace({ tab: 'chats', room: created ?? place.room, grouping: false });
                    }}
                  />
                ) : room !== undefined ? (
                  <ChatView
                    key={room.id}
                    room={room}
                    view={view}
                    game={game}
                    heads={heads}
                    onLeft={() => {
                      show({ room: null });
                    }}
                  />
                ) : (
                  view.rooms.length > 0 && <p className={styles.paneEmpty}>{t.chooseChat}</p>
                )}
              </div>
            </div>
          )}
          {tab === 'friends' && (
            <FriendsTab
              view={view}
              settings={settings}
              game={game}
              heads={heads}
              onOpen={openRoom}
            />
          )}
          {tab === 'requests' && <RequestsTab view={view} heads={heads} />}
        </div>
      </>
    );
  }

  return (
    <section className={styles.sheet} aria-labelledby={headingId}>
      <div className={styles.head}>
        <h2 id={headingId} ref={heading} className={styles.title} tabIndex={-1}>
          {t.open}
        </h2>
        {ready && (
          <Button
            size="small"
            className={styles.square}
            aria-label={t.newGroup}
            title={fewFriends ? t.newGroupNeeds : t.newGroup}
            aria-disabled={fewFriends || undefined}
            onClick={() => {
              if (!fewFriends) onPlace({ tab: 'chats', room: place.room, grouping: true });
            }}
          >
            <Icon name="plus" size={18} />
          </Button>
        )}
        <button
          type="button"
          className={styles.close}
          aria-label={t.close}
          title={t.close}
          onClick={onClose}
        >
          <Icon name="x" size={18} />
        </button>
      </div>
      {view.sanction !== null && <Notice tone="warning">{sanctionText(view.sanction)}</Notice>}
      {view.signedIn && view.problem !== null ? (
        !unconfirmed && <Notice tone="warning">{problemText(view.problem)}</Notice>
      ) : view.signedIn && view.me === null ? (
        <Spinner label={t.connecting} />
      ) : (
        view.signedIn && !view.connected && <Notice tone="info">{t.offline}</Notice>
      )}
      {body}
    </section>
  );
}
