import { type ReactNode, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { SkinHead } from '../../components/SkinHead';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { LauncherAccountProblem, LauncherAccountView } from '../../lib/contracts';
import { accountProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { useOwnSkin } from '../skins/useOwnSkin';
import styles from './Accounts.module.css';

const a = strings.accounts;

interface Props {
  view: LauncherAccountView;
  isDefault: boolean;
  busy: boolean;
  onDefault: () => void;
  /** How the account plays: the demo or offline switch, at the panel's head while
   * signed in… */
  mode?: ReactNode;
  /** …and what that mode means. */
  modeHint?: string;
  /** Opens the Skins tab, where this account's skin and cape are changed. */
  onOpenSkins?: (() => void) | undefined;
}

/** The launcher account (D-064): sign in or create one in its own window, the
 * nick, email confirmation, sign out. Passwords are only typed on oauth.lapi.sh's
 * pages, never here. Its skin and cape live in the Skins tab (D-072). */
export function LauncherAccountPanel({
  view,
  isDefault,
  busy,
  onDefault,
  mode,
  modeHint,
  onOpenSkins,
}: Props) {
  const open = useCommand((_: string, screen: 'login' | 'register' | 'delete') =>
    ipc.launcherAccountOpen(
      screen,
      screen === 'register'
        ? strings.windowTitles.accountRegister
        : screen === 'delete'
          ? strings.windowTitles.accountDelete
          : strings.windowTitles.accountLogin,
    ),
  );
  const [exporting, setExporting] = useState(false);
  const signOut = useCommand(() => ipc.launcherAccountSignOut());
  const setNick = useCommand((_: string, nick: string) => ipc.launcherAccountSetNick(nick));
  const resend = useCommand(() => ipc.launcherAccountResendConfirmation());
  const [editing, setEditing] = useState(false);
  const [nick, setNickText] = useState('');
  const [problem, setProblem] = useState<LauncherAccountProblem | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const shown = problem ?? view.problem;
  const signedIn = view.id !== null && view.nick !== null;
  const texture = useOwnSkin(signedIn && !view.needsSignIn ? view.nick : null);
  const [checking, setChecking] = useState(false);
  const lastCheck = useRef(0);

  /** Asks the server whether the email is confirmed now. Quiet: only the
   * badge changes (the automatic check when the launcher comes back). */
  const recheck = async (quiet: boolean) => {
    lastCheck.current = Date.now();
    if (!quiet) {
      setChecking(true);
      setDone(null);
      setProblem(null);
    }
    try {
      const confirmed = await ipc.launcherAccountRecheck();
      if (!quiet) setDone(confirmed ? a.launcherNowConfirmed : a.launcherStillNotConfirmed);
      else if (confirmed) setDone(a.launcherNowConfirmed);
    } catch (e) {
      if (!quiet) setProblem(accountProblem(e));
    } finally {
      if (!quiet) setChecking(false);
    }
  };

  // Confirming happens in the browser: back in the launcher, look again (at
  // most every 15 seconds).
  const waiting = signedIn && !view.emailVerified && !view.needsSignIn;
  const recheckRef = useRef(recheck);
  useEffect(() => {
    recheckRef.current = recheck;
  });
  useEffect(() => {
    if (!waiting) return;
    const onFocus = () => {
      if (Date.now() - lastCheck.current > 15_000) void recheckRef.current(true);
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.removeEventListener('focus', onFocus);
    };
  }, [waiting]);

  /** Saves the account's data where the player chooses. */
  const exportData = async () => {
    setExporting(true);
    setDone(null);
    setProblem(null);
    try {
      const name = await ipc.launcherAccountExport(strings.windowTitles.saveAccountData);
      if (name !== null) setDone(a.launcherExported({ name }));
    } catch (e) {
      setProblem(
        typeof e === 'string' && e in a.launcherProblems ? (e as LauncherAccountProblem) : 'server',
      );
    } finally {
      setExporting(false);
    }
  };

  const signInButtons = (
    <div className={styles.actions}>
      <Button
        variant="primary"
        disabled={view.signingIn || open.pending}
        onClick={() => void open.run('login')}
      >
        {a.launcherSignIn}
      </Button>
      {!signedIn && (
        <Button disabled={view.signingIn || open.pending} onClick={() => void open.run('register')}>
          {a.launcherRegister}
        </Button>
      )}
    </div>
  );

  return (
    <Panel title={a.launcherTitle} level={3} aside={signedIn ? mode : undefined}>
      <p className={styles.hint}>{a.launcherHint}</p>
      {signedIn && (
        <>
          <div className={styles.person}>
            <SkinHead texture={texture} size={40} />
            <strong className={styles.nick} dir="ltr">
              {view.nick}
            </strong>
            <StatusBadge
              tone={view.emailVerified ? 'success' : 'warning'}
              label={view.emailVerified ? a.launcherEmailConfirmed : a.launcherEmailNotConfirmed}
            />
            {waiting && (
              <button
                type="button"
                className={styles.refresh}
                aria-label={checking ? a.launcherChecking : a.launcherCheck}
                title={a.launcherCheck}
                disabled={checking}
                data-busy={checking ? 'true' : undefined}
                onClick={() => void recheck(false)}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M20 12a8 8 0 1 1-2.34-5.66" />
                  <path d="M20 4v5h-5" />
                </svg>
              </button>
            )}
            {isDefault && <span className={styles.tag}>{a.isDefault}</span>}
          </div>
          {modeHint !== undefined && <p className={styles.hint}>{modeHint}</p>}
          {editing ? (
            <form
              className={forms.form}
              aria-label={a.launcherChangeNick}
              onSubmit={(event) => {
                event.preventDefault();
                setDone(null);
                void setNick.run(nick.trim()).then((result) => {
                  if (result === undefined) return;
                  setProblem(result);
                  if (result === null) {
                    setEditing(false);
                    setDone(a.launcherNickChanged);
                  }
                });
              }}
            >
              <Field label={a.launcherNick} hint={a.nameHint}>
                {(props) => (
                  <input
                    {...props}
                    className={[forms.input, styles.nameInput].join(' ')}
                    dir="ltr"
                    maxLength={16}
                    spellCheck={false}
                    autoComplete="off"
                    value={nick}
                    onChange={(event) => {
                      setNickText(event.target.value);
                      setProblem(null);
                    }}
                  />
                )}
              </Field>
              <div className={styles.actions}>
                <Button
                  type="submit"
                  variant="primary"
                  size="small"
                  disabled={setNick.pending || nick.trim() === ''}
                >
                  {a.launcherSaveNick}
                </Button>
                <Button
                  size="small"
                  onClick={() => {
                    setEditing(false);
                    setProblem(null);
                  }}
                >
                  {a.launcherCancel}
                </Button>
              </div>
            </form>
          ) : (
            <div className={styles.actions}>
              {!isDefault && (
                <Button size="small" disabled={busy} onClick={onDefault}>
                  {a.makeDefault}
                </Button>
              )}
              <Button
                size="small"
                onClick={() => {
                  setNickText(view.nick ?? '');
                  setEditing(true);
                  setDone(null);
                }}
              >
                {a.launcherChangeNick}
              </Button>
              {!view.emailVerified && (
                <Button
                  size="small"
                  disabled={resend.pending}
                  onClick={() => {
                    setDone(null);
                    void resend.run().then((result) => {
                      if (result === undefined) return;
                      setProblem(result);
                      if (result === null) setDone(a.launcherResent);
                    });
                  }}
                >
                  {a.launcherResend}
                </Button>
              )}
              {!view.needsSignIn && (
                <Button size="small" disabled={exporting} onClick={() => void exportData()}>
                  {a.launcherExport}
                </Button>
              )}
              <Button size="small" disabled={signOut.pending} onClick={() => void signOut.run()}>
                {a.launcherSignOut}
              </Button>
              {!view.needsSignIn && (
                <Button
                  variant="ghost"
                  size="small"
                  disabled={view.signingIn || open.pending}
                  onClick={() => void open.run('delete')}
                >
                  {a.launcherDelete}
                </Button>
              )}
            </div>
          )}
          {!view.needsSignIn && <p className={styles.hint}>{a.launcherDeleteHint}</p>}
          {!view.needsSignIn && onOpenSkins !== undefined && (
            <p className={styles.where}>
              <span className={styles.hint}>{a.skinsWhere}</span>
              <button type="button" className={styles.link} onClick={onOpenSkins}>
                {a.openSkins}
              </button>
            </p>
          )}
        </>
      )}
      {view.signingIn && <p role="status">{a.launcherWaiting}</p>}
      {done !== null && <p role="status">{done}</p>}
      {shown !== null && <Notice tone="warning">{a.launcherProblems[shown]}</Notice>}
      {(open.error ?? signOut.error ?? setNick.error ?? resend.error) !== null && (
        <Notice tone="danger">
          {open.error ?? signOut.error ?? setNick.error ?? resend.error}
        </Notice>
      )}
      {(!signedIn || view.needsSignIn) && signInButtons}
    </Panel>
  );
}
