import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import type { AppSnapshot, LocalWorld, WorldsRoom } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { CreateWorld } from './CreateWorld';
import { Devices } from './Devices';
import { ImportWorld } from './ImportWorld';
import { JoinByCode } from './JoinByCode';
import { RelayOnly } from './RelayOnly';
import { RoomActions } from './RoomActions';
import { RoomPage } from './RoomPage';
import { WorldsStatus } from './WorldsStatus';
import { ROOMS_EVERY_MS, roomOpen, useWorldsState, worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
  /** A room to show first, such as one joined from an invite in a chat. */
  initialRoom?: string | null;
}

/**
 * Worlds with friends (D-068): the account's rooms, creating a world, joining
 * with an invite code, a room's page, and what this computer hosts or joins.
 */
export function WorldsView({ snapshot, onOpenAccounts, initialRoom = null }: Props) {
  const t = strings.worlds;
  const signedIn = snapshot.account.launcherAccount.id !== null;
  const state = useWorldsState();
  const [rooms, setRooms] = useState<WorldsRoom[] | null>(null);
  const [local, setLocal] = useState<LocalWorld[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [room, setRoom] = useState<string | null>(initialRoom);
  const [dialog, setDialog] = useState<'create' | 'import' | 'code' | 'devices' | null>(null);

  const load = useCallback(() => {
    void ipc
      .worldsLocal()
      .then(setLocal)
      .catch(() => undefined);
    ipc
      .worldsRooms()
      .then((next) => {
        setRooms(next);
        setError(null);
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
        setRooms((current) => current ?? []);
      });
  }, []);

  useEffect(() => {
    if (signedIn) load();
  }, [signedIn, load]);

  // Friends' rooms open, close and fill meanwhile.
  useEffect(() => {
    if (!signedIn || room !== null) return undefined;
    const timer = window.setInterval(() => {
      void ipc
        .worldsRooms()
        .then(setRooms)
        .catch(() => undefined);
    }, ROOMS_EVERY_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [signedIn, room]);

  // A room opened or closed here shows in the list at once.
  const hostingRoom = state.hosting?.ended === null ? state.hosting.room : null;
  useEffect(() => {
    if (signedIn && hostingRoom !== null) load();
  }, [signedIn, hostingRoom, load]);

  // A world played for the first time, restored or imported shows at once.
  const changing = useRef(state.maintenance);
  useEffect(() => {
    if (changing.current && !state.maintenance) {
      void ipc
        .worldsLocal()
        .then(setLocal)
        .catch(() => undefined);
    }
    changing.current = state.maintenance;
  }, [state.maintenance]);

  const onLocal = (world: LocalWorld) => {
    setLocal((current) => current.map((w) => (w.id === world.id ? world : w)));
  };

  if (!signedIn) {
    return (
      <Panel title={t.title}>
        <Notice tone="info">{t.signIn}</Notice>
        <div className={styles.toolbar}>
          <Button variant="primary" onClick={onOpenAccounts}>
            {t.openAccount}
          </Button>
        </div>
      </Panel>
    );
  }

  return (
    <Panel title={t.title}>
      <WorldsStatus state={state} rooms={rooms ?? []} />
      {room !== null ? (
        <RoomPage
          key={room}
          room={room}
          local={local}
          state={state}
          onLocal={onLocal}
          onBack={() => {
            setRoom(null);
            load();
          }}
          onLeft={() => {
            setRoom(null);
            load();
          }}
        />
      ) : (
        <>
          <div className={styles.toolbar}>
            <Button
              variant="primary"
              onClick={() => {
                setDialog('create');
              }}
            >
              {t.create}
            </Button>
            <Button
              onClick={() => {
                setDialog('import');
              }}
            >
              {t.import.open}
            </Button>
            <Button
              onClick={() => {
                setDialog('code');
              }}
            >
              {t.joinByCode}
            </Button>
            <Button onClick={load}>{t.refresh}</Button>
            <Button
              onClick={() => {
                setDialog('devices');
              }}
            >
              {t.devices.open}
            </Button>
          </div>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <h3 className={styles.sectionTitle}>{t.rooms}</h3>
          {rooms === null ? (
            <p className={styles.muted}>{t.loading}</p>
          ) : rooms.length === 0 ? (
            error === null && <p className={styles.muted}>{t.noRooms}</p>
          ) : (
            <ul className={styles.rooms}>
              {rooms.map((item) => {
                const world = local.find((w) => w.id === item.world) ?? null;
                const now = roomOpen(item, state);
                return (
                  <li key={item.id} className={styles.room}>
                    <span className={styles.roomName}>{item.name}</span>
                    <span className={styles.muted}>
                      {t.roomMeta({
                        owner: item.ownerNick,
                        version: item.minecraft,
                        members: item.members,
                      })}
                    </span>
                    <span className={styles.badges}>
                      <StatusBadge
                        tone={now.open ? 'success' : 'neutral'}
                        label={
                          now.open
                            ? t.openNow({ online: now.online, max: item.maxPlayers })
                            : t.closedNow
                        }
                      />
                      {item.role === 'owner' && (
                        <StatusBadge
                          tone="neutral"
                          label={world === null ? t.keptElsewhere : t.keptHere}
                        />
                      )}
                    </span>
                    <RoomActions room={item} local={world} state={state} onLocal={onLocal} />
                    <div className={styles.actions}>
                      <Button
                        onClick={() => {
                          setRoom(item.id);
                        }}
                      >
                        {t.open}
                      </Button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          <RelayOnly settings={snapshot.settings} />
        </>
      )}
      {dialog === 'create' && (
        <CreateWorld
          onClose={() => {
            setDialog(null);
          }}
          onCreated={() => {
            setDialog(null);
            load();
          }}
        />
      )}
      {dialog === 'import' && (
        <ImportWorld
          onClose={() => {
            setDialog(null);
          }}
          onImported={(imported) => {
            setDialog(null);
            load();
            setRoom(imported.id);
          }}
        />
      )}
      {dialog === 'devices' && (
        <Devices
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
      {dialog === 'code' && (
        <JoinByCode
          onClose={() => {
            setDialog(null);
          }}
          onJoined={(joined) => {
            setDialog(null);
            load();
            setRoom(joined.id);
          }}
        />
      )}
    </Panel>
  );
}
