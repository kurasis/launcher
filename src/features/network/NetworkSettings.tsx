import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, TrafficSummary, TrafficUsage } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Network.module.css';

const t = strings.metered;
const MIB = 1024 * 1024;
/** How often the count refreshes while the section is open. */
const REFRESH_MS = 5000;
const SOURCES: (keyof TrafficUsage)[] = ['game', 'java', 'loaders', 'metadata'];

function total(usage: TrafficUsage): number {
  return usage.game + usage.java + usage.loaders + usage.metadata;
}

/** "October 2026" for `2026-10`. */
function monthName(month: string): string {
  const [year, number] = month.split('-').map(Number);
  if (year === undefined || number === undefined) return month;
  return new Date(year, number - 1, 1).toLocaleDateString(numberLocale(), {
    year: 'numeric',
    month: 'long',
  });
}

function toInt(text: string): number {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN;
}

/**
 * Metered connection settings and the traffic the launcher downloaded (D-051).
 */
export function NetworkSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const { settings } = snapshot;
  const [warn, setWarn] = useState(String(settings.meteredWarnMib));
  const [limit, setLimit] = useState(String(settings.monthlyLimitMib));
  const [saved, setSaved] = useState(false);
  const [traffic, setTraffic] = useState<TrafficSummary | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const update = useCommand(
    (
      requestId: string,
      patch: { metered?: boolean; meteredWarnMib?: number; monthlyLimitMib?: number },
    ) => ipc.setAppSettings(patch, requestId),
  );
  const reset = useCommand((requestId: string) => ipc.resetTraffic(requestId));
  const disabled = update.pending || !settings.writable;

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

  const used = traffic === null ? 0 : total(traffic.thisMonth);
  const limitBytes = settings.monthlyLimitMib * MIB;

  return (
    <div className={styles.network}>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={settings.metered}
          disabled={disabled}
          onChange={(event) => void update.run({ metered: event.target.checked })}
        />
        <span>
          <span className={styles.strong}>{t.toggle}</span>
          <span className={styles.hint}>{t.toggleHint}</span>
        </span>
      </label>
      <form
        className={forms.form}
        aria-label={t.toggle}
        onSubmit={(event) => {
          event.preventDefault();
          setSaved(false);
          void update
            .run({ meteredWarnMib: toInt(warn), monthlyLimitMib: toInt(limit) })
            .then((done) => {
              if (done) setSaved(true);
            });
        }}
      >
        <Field label={t.warn} hint={t.warnHint}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={0}
              value={warn}
              disabled={disabled}
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
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={0}
              value={limit}
              disabled={disabled}
              onChange={(event) => {
                setLimit(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
        {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
        <div className={forms.actions}>
          <Button type="submit" disabled={disabled}>
            {t.save}
          </Button>
          {saved && <span role="status">{t.saved}</span>}
        </div>
      </form>

      <section className={styles.usage} aria-label={t.usage}>
        <h3 className={styles.title}>{t.usage}</h3>
        {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
        {traffic !== null && (
          <>
            <p className={styles.strong}>
              {settings.monthlyLimitMib > 0
                ? t.usedOfLimit({ used: formatBytes(used), limit: formatBytes(limitBytes) })
                : t.used({ used: formatBytes(used) })}
            </p>
            {settings.monthlyLimitMib > 0 && (
              <progress
                className={styles.bar}
                aria-label={t.usage}
                max={limitBytes}
                value={Math.min(used, limitBytes)}
                data-over={used > limitBytes ? 'true' : undefined}
              />
            )}
            <dl className={styles.sources}>
              {SOURCES.map((source) => (
                <div key={source}>
                  <dt>{t.sources[source]}</dt>
                  <dd>{formatBytes(traffic.thisMonth[source])}</dd>
                </div>
              ))}
            </dl>
            {traffic.earlier.length > 0 && (
              <>
                <h4 className={styles.subtitle}>{t.earlier}</h4>
                <dl className={styles.sources}>
                  {traffic.earlier.map((month) => (
                    <div key={month.month}>
                      <dt>{monthName(month.month)}</dt>
                      <dd>{formatBytes(month.bytes)}</dd>
                    </div>
                  ))}
                </dl>
              </>
            )}
            <p className={styles.hint}>{t.notCounted}</p>
            {confirming ? (
              <Notice tone="warning">
                <p>{t.confirmReset}</p>
                <div className={forms.actions}>
                  <Button
                    variant="primary"
                    disabled={reset.pending}
                    onClick={() => {
                      void reset.run().then((found) => {
                        setConfirming(false);
                        if (found) setTraffic(found);
                      });
                    }}
                  >
                    {t.reset}
                  </Button>
                  <Button
                    onClick={() => {
                      setConfirming(false);
                    }}
                  >
                    {t.cancel}
                  </Button>
                </div>
              </Notice>
            ) : (
              <div className={forms.actions}>
                <Button
                  onClick={() => {
                    setConfirming(true);
                  }}
                >
                  {t.reset}
                </Button>
              </div>
            )}
            {reset.error !== null && <Notice tone="danger">{reset.error}</Notice>}
          </>
        )}
      </section>
    </div>
  );
}
