import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { ModsImportRequest } from '../../lib/contracts';
import { describeIpcError, ipc, isLauncherError } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { Dialog } from './Dialog';
import { compact, failureText, ourPicture, stateTone } from './helpers';
import styles from './Mods.module.css';

/** How often a request in progress is asked about. */
export const POLL_MS = 3000;

/** A sign-in button when the error is that the launcher account is signed out. */
export function SignInNotice({ error }: { error: unknown }) {
  const signedOut = isLauncherError(error) && error.code === 'AUTH_REAUTH_REQUIRED';
  return (
    <Notice tone="danger">
      <p>{describeIpcError(error)}</p>
      {signedOut && (
        <Button
          onClick={() => {
            void ipc
              .launcherAccountOpen('login', strings.windowTitles.accountLogin)
              .catch(() => undefined);
          }}
        >
          {strings.mods.requests.signIn}
        </Button>
      )}
    </Notice>
  );
}

/** Follows one request: its state, a choice among candidates, the result. */
export function RequestProgress({
  request,
  onChange,
  onOpen,
}: {
  request: ModsImportRequest;
  onChange: (request: ModsImportRequest) => void;
  onOpen: (projectId: string) => void;
}) {
  const t = strings.mods.requests;
  const [error, setError] = useState<unknown>(null);
  const [choosing, setChoosing] = useState(false);
  const waiting = request.state === 'queued' || request.state === 'importing';
  const id = request.id;

  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => {
      void ipc
        .modsImportState(id)
        .then(onChange)
        .catch(() => undefined);
    }, POLL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [waiting, id, onChange]);

  return (
    <div className={styles.request}>
      <p className={styles.cardTitle}>
        <bdi>{request.input}</bdi>
        <StatusBadge tone={stateTone(request.state)} label={t.states[request.state]} />
      </p>
      {waiting && <p className={styles.muted}>{t.waitingHint}</p>}
      {request.state === 'failed' && <p>{failureText(request.error)}</p>}
      {request.state === 'choosing' && (
        <>
          <p>{t.choose}</p>
          <ul className={styles.candidates}>
            {request.candidates.map((c) => {
              const banner = ourPicture(c.banner);
              const icon = ourPicture(c.icon);
              return (
                <li key={`${c.source}:${c.id}`} className={styles.candidate}>
                  {banner !== null && (
                    <img className={styles.banner} src={banner} alt="" loading="lazy" />
                  )}
                  {icon !== null ? (
                    <img className={styles.icon} src={icon} alt="" loading="lazy" />
                  ) : (
                    <span className={styles.icon} aria-hidden="true" />
                  )}
                  <span className={styles.cardBody}>
                    <span className={styles.cardTitle}>
                      <bdi>{c.title || c.id}</bdi>
                      <span className={styles.source}>{strings.mods.sources[c.source]}</span>
                    </span>
                    {c.author !== '' && (
                      <span className={styles.muted}>{strings.mods.by({ author: c.author })}</span>
                    )}
                    <span className={styles.summary}>{c.summary}</span>
                    <span className={styles.meta}>
                      {strings.mods.downloads({ count: compact(c.downloads) })}
                    </span>
                  </span>
                  {c.projectId !== null ? (
                    <Button
                      onClick={() => {
                        if (c.projectId !== null) onOpen(c.projectId);
                      }}
                    >
                      {t.inCatalog}
                    </Button>
                  ) : (
                    <Button
                      variant="primary"
                      disabled={choosing}
                      onClick={() => {
                        setChoosing(true);
                        setError(null);
                        void ipc
                          .modsChoose(id, c.source, c.id)
                          .then(onChange)
                          .catch(setError)
                          .finally(() => {
                            setChoosing(false);
                          });
                      }}
                    >
                      {t.pick}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
      {request.projectId !== null && request.state !== 'choosing' && (
        <div className={forms.actions}>
          <Button
            variant={request.state === 'ready' ? 'primary' : 'secondary'}
            onClick={() => {
              if (request.projectId !== null) onOpen(request.projectId);
            }}
          >
            {t.open}
          </Button>
        </div>
      )}
      {error !== null && <SignInNotice error={error} />}
    </div>
  );
}

/**
 * "Add to the catalog": a link to a project on Modrinth or CurseForge, or a name (D-066).
 * The dialog can be closed; the request stays in "My requests".
 */
export function AddToCatalog({
  initial,
  onClose,
  onOpen,
}: {
  initial: string;
  onClose: () => void;
  onOpen: (projectId: string) => void;
}) {
  const t = strings.mods.requests;
  const [input, setInput] = useState(initial);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [request, setRequest] = useState<ModsImportRequest | null>(null);
  const text = input.trim();
  return (
    <Dialog title={strings.mods.addToCatalog} onClose={onClose} wide>
      {request === null ? (
        <form
          className={forms.form}
          onSubmit={(event) => {
            event.preventDefault();
            if (text === '') return;
            setSending(true);
            setError(null);
            void ipc
              .modsImport(text)
              .then(setRequest)
              .catch(setError)
              .finally(() => {
                setSending(false);
              });
          }}
        >
          <Field label={t.input} hint={t.inputHint}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                value={input}
                maxLength={512}
                onChange={(event) => {
                  setInput(event.target.value);
                }}
              />
            )}
          </Field>
          {error !== null && <SignInNotice error={error} />}
          <div className={forms.actions}>
            <Button type="submit" variant="primary" disabled={sending || text === ''}>
              {t.send}
            </Button>
            <Button onClick={onClose}>{t.close}</Button>
          </div>
        </form>
      ) : (
        <>
          <RequestProgress request={request} onChange={setRequest} onOpen={onOpen} />
          <p className={styles.muted}>{t.closeHint}</p>
          <div className={forms.actions}>
            <Button onClick={onClose}>{t.close}</Button>
          </div>
        </>
      )}
    </Dialog>
  );
}

/** "My requests": what the launcher account asked to add, newest first. */
export function RequestsSection({ onOpen }: { onOpen: (projectId: string) => void }) {
  const t = strings.mods.requests;
  const [requests, setRequests] = useState<ModsImportRequest[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const fetchAll = useCallback(() => {
    void ipc
      .modsImports()
      .then((list) => {
        setError(null);
        setRequests(list);
      })
      .catch(setError);
  }, []);
  useEffect(fetchAll, [fetchAll]);
  const replace = useCallback((next: ModsImportRequest) => {
    setRequests((list) => list?.map((r) => (r.id === next.id ? next : r)) ?? null);
  }, []);
  return (
    <div className={styles.section}>
      <p className={styles.muted}>{t.hint}</p>
      {error !== null && <SignInNotice error={error} />}
      {requests === null ? (
        error === null && <p role="status">{strings.mods.loading}</p>
      ) : requests.length === 0 ? (
        <p className={styles.muted}>{t.none}</p>
      ) : (
        <ul className={styles.requests}>
          {requests.map((request) => (
            <li key={request.id}>
              <RequestProgress request={request} onChange={replace} onOpen={onOpen} />
            </li>
          ))}
        </ul>
      )}
      <div className={forms.actions}>
        <Button onClick={fetchAll}>{t.refresh}</Button>
      </div>
    </div>
  );
}
