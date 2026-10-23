import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { ProgressBar } from '../../components/ProgressBar';
import { Switch } from '../../components/Switch';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, TrafficSummary, TrafficUsage } from '../../lib/contracts';
import { formatBytes, toInt } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { trafficTotal } from '../../lib/traffic';
import { useCommand } from '../../lib/useCommand';
import styles from './Network.module.css';

const t = strings.metered;
const MIB = 1024 * 1024;
/** How often the count refreshes while the section is open. */
const REFRESH_MS = 5000;
const SOURCES: (keyof TrafficUsage)[] = ['game', 'java', 'loaders', 'metadata', 'mods'];

/** "October 2026" for `2026-10`. */
function monthName(month: string): string {
  const [year, number] = month.split('-').map(Number);
  if (year === undefined || number === undefined) return month;
  return new Date(year, number - 1, 1).toLocaleDateString(numberLocale(), {
    year: 'numeric',
    month: 'long',
  });
}

/**
 * Metered connection settings and the traffic the launcher downloaded (D-051): the
 * switch and its limits, then this month by source, with earlier months on a press.
 */
export function NetworkSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const { settings } = snapshot;
  const [warn, setWarn] = useState(String(settings.meteredWarnMib));
  const [limit, setLimit] = useState(String(settings.monthlyLimitMib));
  const [saved, setSaved] = useState(false);
  const update = useCommand(
    (
      requestId: string,
      patch: { metered?: boolean; meteredWarnMib?: number; monthlyLimitMib?: number },
    ) => ipc.setAppSettings(patch, requestId),
  );
  // Saving keeps the fields and Save as they are (a disabled control drops the focus),
  // but they do not change until it is done: Saved is said of what was sent.
  const disabled = !settings.writable;

  return (
    <>
      <Panel>
        <Switch
          label={t.toggle}
          hint={t.toggleHint}
          checked={settings.metered}
          disabled={disabled}
          pending={update.pending}
          onChange={(metered) => void update.run({ metered })}
        />
        <form
          className={styles.form}
          onSubmit={(event) => {
            event.preventDefault();
            if (update.pending) return;
            setSaved(false);
            void update
              .run({ meteredWarnMib: toInt(warn), monthlyLimitMib: toInt(limit) })
              .then((done) => {
                if (done) setSaved(true);
              });
          }}
        >
          <div className={styles.fields}>
            <Field label={t.warn} hint={t.warnHint}>
              {(props) => (
                <input
                  {...props}
                  className={[forms.input, styles.number].join(' ')}
                  type="number"
                  inputMode="numeric"
                  min={0}
                  dir="ltr"
                  value={warn}
                  disabled={disabled}
                  readOnly={update.pending}
                  onChange={(event) => {
                    setWarn(event.target.value);
                    setSaved(false);
                  }}
                />
              )}
            </Field>
            <Field label={t.limit} hint={t.limitHint}>
              {(props) => (
                <input
                  {...props}
                  className={[forms.input, styles.number].join(' ')}
                  type="number"
                  inputMode="numeric"
                  min={0}
                  dir="ltr"
                  value={limit}
                  disabled={disabled}
                  readOnly={update.pending}
                  onChange={(event) => {
                    setLimit(event.target.value);
                    setSaved(false);
                  }}
                />
              )}
            </Field>
          </div>
          {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
          <div className={styles.actions}>
            <Button
              type="submit"
              variant="primary"
              size="small"
              disabled={disabled}
              aria-disabled={update.pending || undefined}
            >
              {t.save}
            </Button>
            {saved && (
              <span role="status" className={styles.hint}>
                {t.saved}
              </span>
            )}
          </div>
        </form>
      </Panel>
      <Traffic limitMib={settings.monthlyLimitMib} />
    </>
  );
}

/** This month's traffic by source, earlier months, and resetting the count. */
function Traffic({ limitMib }: { limitMib: number }) {
  const [traffic, setTraffic] = useState<TrafficSummary | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [earlierShown, setEarlierShown] = useState(false);
  const reset = useCommand((requestId: string) => ipc.resetTraffic(requestId));
  const earlierId = useId();
  // The question replaces Reset the count: the focus comes back to it when it closes.
  const resetButton = useRef<HTMLButtonElement>(null);
  useFocusBack(confirming, resetButton);

  const reload = useCallback(() => {
    ipc
      .getTraffic()
      .then((found) => {
        setTraffic(found);
        setLoadError(null);
      })
      .catch((error: unknown) => {
        setLoadError(describeIpcError(error));
      });
  }, []);
  useEffect(() => {
    reload();
    const timer = setInterval(reload, REFRESH_MS);
    return () => {
      clearInterval(timer);
    };
  }, [reload]);

  const used = traffic === null ? 0 : trafficTotal(traffic.thisMonth);
  const limitBytes = limitMib * MIB;

  return (
    <Panel title={t.usage} level={3}>
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {traffic !== null && (
        <>
          <p className={styles.used}>
            {limitMib > 0
              ? t.usedOfLimit({ used: formatBytes(used), limit: formatBytes(limitBytes) })
              : t.used({ used: formatBytes(used) })}
          </p>
          {limitMib > 0 && (
            <ProgressBar
              className={[styles.total, used > limitBytes && styles.over].filter(Boolean).join(' ')}
              value={Math.min(used, limitBytes)}
              max={limitBytes}
              label={t.usage}
            />
          )}
          <ul className={styles.sources}>
            {SOURCES.map((source) => (
              <li key={source} className={styles.source}>
                <span>{t.sources[source]}</span>
                {/* Each source's share of the month; its size says the same in words. */}
                <span aria-hidden="true">
                  <ProgressBar
                    value={traffic.thisMonth[source]}
                    max={used > 0 ? used : 1}
                    label={t.sources[source]}
                  />
                </span>
                <span className={styles.size}>{formatBytes(traffic.thisMonth[source])}</span>
              </li>
            ))}
          </ul>
          <p className={styles.hint}>{t.notCounted}</p>
          <div className={styles.actions}>
            {traffic.earlier.length > 0 && (
              <Button
                size="small"
                aria-expanded={earlierShown}
                aria-controls={earlierId}
                onClick={() => {
                  setEarlierShown(!earlierShown);
                }}
              >
                {t.earlier}
              </Button>
            )}
            {!confirming && (
              <Button
                ref={resetButton}
                variant="ghost"
                size="small"
                onClick={() => {
                  setConfirming(true);
                }}
              >
                {t.reset}
              </Button>
            )}
          </div>
          {traffic.earlier.length > 0 && (
            <ul id={earlierId} className={styles.sources} hidden={!earlierShown}>
              {traffic.earlier.map((month) => (
                <li key={month.month} className={styles.month}>
                  <span>{monthName(month.month)}</span>
                  <span className={styles.size}>{formatBytes(month.bytes)}</span>
                </li>
              ))}
            </ul>
          )}
          {confirming && (
            <Confirm
              label={t.reset}
              text={t.confirmReset}
              action={t.reset}
              cancel={t.cancel}
              pending={reset.pending}
              onYes={() => {
                void reset.run().then((found) => {
                  setConfirming(false);
                  if (found) setTraffic(found);
                });
              }}
              onNo={() => {
                setConfirming(false);
              }}
            />
          )}
          {reset.error !== null && <Notice tone="danger">{reset.error}</Notice>}
        </>
      )}
    </Panel>
  );
}
