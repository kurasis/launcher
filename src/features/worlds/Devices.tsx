import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { WorldsDevice } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { Dialog } from '../../components/Dialog';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

interface Props {
  onClose: () => void;
}

/**
 * The account's computers for worlds. Another computer can be signed out:
 * the worlds it hosts close, and it registers again at its next sign-in.
 */
export function Devices({ onClose }: Props) {
  const t = strings.worlds.devices;
  const [devices, setDevices] = useState<WorldsDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<WorldsDevice | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    ipc
      .worldsDevices()
      .then((next) => {
        setDevices(next);
        setError(null);
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
        setDevices((current) => current ?? []);
      });
  }, []);

  useEffect(load, [load]);

  const revoke = (device: WorldsDevice) => {
    setBusy(true);
    setError(null);
    ipc
      .worldsRevokeDevice(device.id)
      .then(load)
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
        setConfirming(null);
      });
  };

  return (
    <Dialog title={t.title} onClose={onClose} wide>
      <p className={styles.muted}>{t.hint}</p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {devices === null ? (
        <p className={styles.muted}>{strings.worlds.loading}</p>
      ) : (
        <ul className={styles.rooms} aria-label={t.title}>
          {devices.map((device) => (
            <li key={device.id} className={styles.room}>
              <span className={styles.roomName} dir="auto">
                {device.name}
              </span>
              <span className={styles.muted}>
                {device.current
                  ? t.current
                  : t.added({
                      date: new Date(device.created * 1000).toLocaleDateString(numberLocale()),
                    })}
              </span>
              {!device.current && (
                <div className={styles.actions}>
                  {confirming?.id === device.id ? (
                    <>
                      <span>{t.confirm({ name: device.name })}</span>
                      <Button
                        onClick={() => {
                          setConfirming(null);
                        }}
                      >
                        {strings.worlds.cancel}
                      </Button>
                      <Button
                        variant="primary"
                        disabled={busy}
                        onClick={() => {
                          revoke(device);
                        }}
                      >
                        {t.revoke}
                      </Button>
                    </>
                  ) : (
                    <Button
                      onClick={() => {
                        setConfirming(device);
                      }}
                    >
                      {t.revoke}
                    </Button>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className={forms.actions}>
        <Button onClick={onClose}>{strings.worlds.close}</Button>
      </div>
    </Dialog>
  );
}
