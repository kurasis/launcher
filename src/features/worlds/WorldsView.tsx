import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { useFocusBack } from '../../components/useFocusBack';
import type { AppSnapshot, LocalWorld, WorldsImportable, WorldsRoom } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { CreateWorld } from './CreateWorld';
import { Devices } from './Devices';
import { ImportWorld } from './ImportWorld';
import { JoinByCode } from './JoinByCode';
import { RelayOnly } from './RelayOnly';
import { RoomPage } from './RoomPage';
import { RoomRow } from './RoomRow';
import { SinglePlayerWorlds } from './SinglePlayerWorlds';
import { WorldsStatus } from './WorldsStatus';
import { useRoomActions } from './useRoomActions';
import { ROOMS_EVERY_MS, activeRoom, useWorldsState, worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  snapshot: AppSnapshot;
  onOpenAccounts: () => void;
  /** A room to show first, such as one joined from an invite in a chat. */
  initialRoom?: string | null;
}

type Dialog =
  | { kind: 'create' | 'code' | 'devices' }
  | { kind: 'import'; initial?: { instanceId: string; save: string } };

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
  const [saves, setSaves] = useState<WorldsImportable[] | null>(null);
  const [savesError, setSavesError] = useState<string | null>(null);
  const [room, setRoom] = useState<string | null>(initialRoom);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  // The room whose page was left: its Open takes the focus back in the list.
  const [cameFrom, setCameFrom] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const band = useRef<HTMLElement>(null);
  const active = activeRoom(state);
  // The list or a room page takes the place of the button pressed: the title has the
  // focus. A room this computer starts, or stops, hands it to the band, or the title.
  useFocusBack(room, heading);
  useFocusBack(active, band, heading);

  const onLocal = (world: LocalWorld) => {
    setLocal((current) => current.map((w) => (w.id === world.id ? world : w)));
  };
  // What was pressed for the room this computer is in goes on between the list and a
  // room's page: Play waiting for the mods, the mods question, what failed.
  const activeEntry = rooms?.find((r) => r.id === active) ?? null;
  const bandActions = useRoomActions({
    roomId: active ?? '',
    room: activeEntry,
    local: local.find((w) => w.id === activeEntry?.world) ?? null,
    onLocal,
  });

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

  // Every installation's saves folder is read: on opening, on Refresh and after a
  // world was imported, never on the rooms' timer.
  const loadSaves = useCallback(() => {
    ipc
      .worldsImportable()
      .then((list) => {
        setSaves(Array.isArray(list) ? list : []);
        setSavesError(null);
      })
      .catch((e: unknown) => {
        setSavesError(worldsErrorText(e));
        setSaves((current) => current ?? []);
      });
  }, []);

  useEffect(() => {
    if (signedIn) load();
  }, [signedIn, load]);

  useEffect(() => {
    if (signedIn && room === null) loadSaves();
  }, [signedIn, room, loadSaves]);

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

  // A world played for the first time, restored, imported, or with its mods updated or
  // rolled back (that gets its room ready) shows at once.
  const busy = state.maintenance || state.preparing !== null;
  const changing = useRef(busy);
  useEffect(() => {
    if (changing.current && !busy) {
      void ipc
        .worldsLocal()
        .then(setLocal)
        .catch(() => undefined);
    }
    changing.current = busy;
  }, [busy]);

  if (!signedIn) {
    return (
      <Screen title={t.title}>
        <div className={styles.signIn}>
          <Notice tone="info">{t.signIn}</Notice>
          <Button variant="primary" onClick={onOpenAccounts}>
            {t.openAccount}
          </Button>
        </div>
      </Screen>
    );
  }

  if (room !== null) {
    return (
      <RoomPage
        key={room}
        room={room}
        listed={rooms?.find((r) => r.id === room) ?? null}
        rooms={rooms ?? []}
        me={snapshot.account.launcherAccount.id}
        local={local}
        state={state}
        bandActions={bandActions}
        bandRef={band}
        headingRef={heading}
        onLocal={onLocal}
        onBack={() => {
          setCameFrom(room);
          setRoom(null);
          load();
        }}
        onLeft={() => {
          setCameFrom(null);
          setRoom(null);
          load();
        }}
      />
    );
  }

  return (
    <Screen title={t.title} headingRef={heading}>
      <div className={styles.actionRow}>
        <Button
          variant="primary"
          onClick={() => {
            setDialog({ kind: 'create' });
          }}
        >
          <Icon name="plus" size={18} />
          {t.create}
        </Button>
        <Button
          onClick={() => {
            setDialog({ kind: 'import' });
          }}
        >
          {t.import.open}
        </Button>
        <Button
          onClick={() => {
            setDialog({ kind: 'code' });
          }}
        >
          <Icon name="key" size={18} />
          {t.joinByCode}
        </Button>
        <Button
          onClick={() => {
            load();
            loadSaves();
          }}
        >
          <Icon name="refresh" size={18} />
          {t.refresh}
        </Button>
        <Button
          onClick={() => {
            setDialog({ kind: 'devices' });
          }}
        >
          {t.devices.open}
        </Button>
      </div>
      <WorldsStatus state={state} rooms={rooms ?? []} actions={bandActions} bandRef={band} />
      <div className={styles.columns}>
        <Panel title={t.rooms} level={3} count={rooms === null ? undefined : rooms.length}>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          {rooms === null ? (
            <p className={styles.muted}>{t.loading}</p>
          ) : rooms.length === 0 ? (
            error === null && <p className={styles.muted}>{t.noRooms}</p>
          ) : (
            <ul className={styles.roomList}>
              {rooms.map((item) => (
                <RoomRow
                  key={item.id}
                  room={item}
                  local={local.find((w) => w.id === item.world) ?? null}
                  state={state}
                  active={item.id === active}
                  cameFrom={item.id === cameFrom}
                  onLocal={onLocal}
                  onOpen={() => {
                    setRoom(item.id);
                  }}
                />
              ))}
            </ul>
          )}
        </Panel>
        <div className={styles.column}>
          <SinglePlayerWorlds
            sources={saves}
            error={savesError}
            onImport={(instanceId, save) => {
              setDialog({ kind: 'import', initial: { instanceId, save } });
            }}
          />
          <RelayOnly settings={snapshot.settings} />
        </div>
      </div>
      {dialog?.kind === 'create' && (
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
      {dialog?.kind === 'import' && (
        <ImportWorld
          // A world chosen in the list comes with the list read already.
          known={dialog.initial === undefined || saves === null ? undefined : saves}
          initial={dialog.initial}
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
      {dialog?.kind === 'devices' && (
        <Devices
          onClose={() => {
            setDialog(null);
          }}
        />
      )}
      {dialog?.kind === 'code' && (
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
    </Screen>
  );
}
