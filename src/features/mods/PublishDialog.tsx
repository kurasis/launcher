import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  InstanceSummary,
  ModsOwnPack,
  ModsPublishCheck,
  ModsPublished,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc, onModsProgress } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from '../../components/Dialog';
import styles from './Mods.module.css';

/** Paths shown in one list; the check names at most 50. */
const SHOWN = 10;

function Paths({ title, paths }: { title: string; paths: string[] }) {
  if (paths.length === 0) return null;
  return (
    <>
      <p>{title}</p>
      <ul>
        {paths.slice(0, SHOWN).map((path) => (
          <li key={path}>
            <bdi dir="ltr">{path}</bdi>
          </li>
        ))}
        {paths.length > SHOWN && (
          <li>{strings.mods.project.andMore({ count: paths.length - SHOWN })}</li>
        )}
      </ul>
    </>
  );
}

/**
 * Publishing an installation as the player's own pack of the catalog
 * (D-066 stage 6): the launcher sorts its files first, the player fills in
 * the pack's page, and the catalog checks it all again. A pack published
 * before gets a new version, unless the player asks for a new pack.
 */
export function PublishDialog({
  instance,
  onClose,
  onOpen,
}: {
  instance: InstanceSummary;
  onClose: () => void;
  onOpen: (projectId: string) => void;
}) {
  const t = strings.mods.content;
  const [worlds, setWorlds] = useState(false);
  const [options, setOptions] = useState(false);
  const [check, setCheck] = useState<ModsPublishCheck | null>(null);
  // The files choice the last check answered for; another choice checks again.
  const [checkedFor, setCheckedFor] = useState<string | null>(null);
  const [pack, setPack] = useState<ModsOwnPack | null>(null);
  const [asNew, setAsNew] = useState(false);
  const [title, setTitle] = useState<string | null>(null);
  const [summary, setSummary] = useState('');
  const [body, setBody] = useState('');
  const [version, setVersion] = useState<string | null>(null);
  const [changes, setChanges] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ModsPublished | null>(null);
  // How much of the pack has gone to the catalog.
  const [sending, setSending] = useState<{ bytes: number; total: number } | null>(null);

  useEffect(
    () =>
      onModsProgress((p) => {
        if (p.instanceId === instance.id && p.phase === 'uploading') {
          setSending({ bytes: p.bytes, total: p.totalBytes });
        }
      }),
    [instance.id],
  );

  const choiceKey = `${String(worlds)}/${String(options)}`;
  const checking = checkedFor !== choiceKey;
  useEffect(() => {
    let live = true;
    ipc
      .modsPublishCheck(instance.id, { worlds, options })
      .then((found) => {
        if (!live) return;
        setCheck(found);
        setPack(found.pack);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(describeIpcError(e));
      })
      .finally(() => {
        if (live) setCheckedFor(`${String(worlds)}/${String(options)}`);
      });
    return () => {
      live = false;
    };
  }, [instance.id, worlds, options]);

  // A pack the catalog's owner removed takes no new versions.
  const removed = pack?.status === 'removed';
  const newPack = pack === null || asNew || removed;
  const shownTitle = title ?? (newPack ? (check?.name ?? '') : pack.title);
  const shownVersion = version ?? (newPack ? '1.0.0' : '');
  const ready =
    check !== null &&
    check.check.missing.length === 0 &&
    check.check.refused.length === 0 &&
    check.check.tooBig.length === 0;
  const published = result?.outcome === 'published' ? result : null;
  const refused = result?.outcome === 'refused' ? result : null;

  const pickIcon = () => {
    setError(null);
    ipc
      .modsPublishIcon(t.publishIcon)
      .then((picked) => {
        if (picked !== null) setIcon(picked.preview);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      });
  };

  const toggleHidden = () => {
    if (pack === null) return;
    setWorking(true);
    setError(null);
    ipc
      .modsPackHidden(pack.id, pack.status !== 'hidden')
      .then((status) => {
        setPack({ ...pack, status });
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setWorking(false);
      });
  };

  const start = () => {
    setWorking(true);
    setError(null);
    setResult(null);
    setSending(null);
    ipc
      .modsPublish({
        instanceId: instance.id,
        form: {
          title: shownTitle,
          summary,
          body,
          version: shownVersion,
          changes,
          choice: { worlds, options },
        },
        icon: icon !== null,
        asNew: newPack,
      })
      .then((answer) => {
        setResult(answer);
        if (answer.outcome === 'published') setIcon(null);
      })
      .catch((e: unknown) => {
        setError(describeIpcError(e));
      })
      .finally(() => {
        setWorking(false);
      });
  };
  const close = () => {
    if (!working) onClose();
  };

  const refusals: Record<string, string | undefined> = t.publishRefused;
  return (
    <Dialog title={t.publishTitle} onClose={close}>
      <p className={styles.muted}>{t.publishHint}</p>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={worlds}
          disabled={working}
          onChange={(event) => {
            setWorlds(event.target.checked);
          }}
        />
        {t.exportWorlds}
      </label>
      <label className={styles.check}>
        <input
          type="checkbox"
          checked={options}
          disabled={working}
          onChange={(event) => {
            setOptions(event.target.checked);
          }}
        />
        {t.exportOptions}
      </label>
      {checking && <p role="status">{t.publishChecking}</p>}
      {!checking && check !== null && ready && (
        <p className={styles.muted} role="status">
          {t.publishReady({
            linked: check.check.linked,
            bundled: check.check.bundled,
            size: formatBytes(check.check.bytes),
          })}
        </p>
      )}
      {!checking && check !== null && !ready && (
        <Notice tone="warning">
          <Paths title={t.publishMissing} paths={check.check.missing} />
          <Paths title={t.publishNotAllowed} paths={check.check.refused} />
          <Paths title={t.publishTooBig} paths={check.check.tooBig} />
        </Notice>
      )}

      {pack !== null && (
        <Notice tone="info">
          <p>
            {t.publishedAs({
              title: pack.title,
              count: pack.versions,
              latest: pack.latest ?? '',
            })}
          </p>
          {pack.status === 'hidden' && <p>{t.publishHidden}</p>}
          {removed && <p>{t.publishRemoved}</p>}
          <div className={forms.actions}>
            <Button
              disabled={working}
              onClick={() => {
                onOpen(pack.id);
              }}
            >
              {t.open}
            </Button>
            {!removed && (
              <Button disabled={working} onClick={toggleHidden}>
                {pack.status === 'hidden' ? t.publishShow : t.publishHide}
              </Button>
            )}
          </div>
          {!removed && (
            <label className={styles.check}>
              <input
                type="checkbox"
                checked={asNew}
                disabled={working}
                onChange={(event) => {
                  setAsNew(event.target.checked);
                  setTitle(null);
                  setVersion(null);
                }}
              />
              {t.publishAsNew}
            </label>
          )}
        </Notice>
      )}

      <Field label={t.publishName}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            value={shownTitle}
            maxLength={80}
            disabled={working}
            onChange={(e) => {
              setTitle(e.target.value);
            }}
          />
        )}
      </Field>
      <Field label={t.publishVersion} hint={t.publishVersionHint}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            value={shownVersion}
            maxLength={48}
            disabled={working}
            onChange={(e) => {
              setVersion(e.target.value);
            }}
          />
        )}
      </Field>
      {newPack ? (
        <>
          <Field label={t.publishSummary}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                value={summary}
                maxLength={300}
                disabled={working}
                onChange={(e) => {
                  setSummary(e.target.value);
                }}
              />
            )}
          </Field>
          <Field label={t.publishBody} hint={t.publishBodyHint}>
            {(props) => (
              <textarea
                {...props}
                className={forms.input}
                value={body}
                maxLength={65536}
                rows={5}
                disabled={working}
                onChange={(e) => {
                  setBody(e.target.value);
                }}
              />
            )}
          </Field>
        </>
      ) : (
        <Field label={t.publishChanges}>
          {(props) => (
            <textarea
              {...props}
              className={forms.input}
              value={changes}
              maxLength={16384}
              rows={4}
              disabled={working}
              onChange={(e) => {
                setChanges(e.target.value);
              }}
            />
          )}
        </Field>
      )}
      <div className={forms.actions}>
        {icon !== null && <img className={styles.icon} src={icon} alt={t.publishIconPreview} />}
        <Button disabled={working} onClick={pickIcon}>
          {icon === null ? t.publishIcon : t.publishIconAgain}
        </Button>
      </div>

      {working && (
        <p role="status">
          {sending === null
            ? t.publishing
            : t.publishSending({
                sent: formatBytes(sending.bytes),
                total: formatBytes(sending.total),
              })}
        </p>
      )}
      {published !== null && (
        <Notice tone="info">
          <p>{t.publishDone}</p>
          <div className={forms.actions}>
            <Button
              onClick={() => {
                onOpen(published.projectId);
              }}
            >
              {t.open}
            </Button>
          </div>
        </Notice>
      )}
      {refused !== null && (
        <Notice tone="danger">
          <Paths title={refusals[refused.code] ?? t.publishRefused.other} paths={refused.paths} />
          {refused.paths.length === 0 && <p>{refusals[refused.code] ?? t.publishRefused.other}</p>}
        </Notice>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        <Button
          variant="primary"
          disabled={working || checking || !ready || published !== null}
          onClick={start}
        >
          {newPack ? t.publishStart : t.publishVersionStart}
        </Button>
        {!working && <Button onClick={onClose}>{t.close}</Button>}
      </div>
    </Dialog>
  );
}
