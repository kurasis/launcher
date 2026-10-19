import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { LauncherAccountProblem, LauncherAccountView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Accounts.module.css';

const a = strings.accounts;

interface Props {
  view: LauncherAccountView;
  isDefault: boolean;
  busy: boolean;
  onDefault: () => void;
}

/** The launcher account (D-064): sign in or create one in its own window, the
 * nick, email confirmation, sign out. Passwords are only typed on oauth.lapi.sh's
 * pages, never here. */
export function LauncherAccountPanel({ view, isDefault, busy, onDefault }: Props) {
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
    <div className={forms.actions}>
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
    <Panel title={a.launcherTitle}>
      <p className={styles.hint}>{a.launcherHint}</p>
      {signedIn && (
        <div className={styles.row}>
          <div className={styles.rowHead}>
            <strong dir="ltr">{view.nick}</strong>
            {isDefault && <span className={styles.tag}>{a.isDefault}</span>}
            <StatusBadge
              tone={view.emailVerified ? 'success' : 'warning'}
              label={view.emailVerified ? a.launcherEmailConfirmed : a.launcherEmailNotConfirmed}
            />
          </div>
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
                    className={forms.input}
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
              <div className={forms.actions}>
                <Button
                  type="submit"
                  variant="primary"
                  disabled={setNick.pending || nick.trim() === ''}
                >
                  {a.launcherSaveNick}
                </Button>
                <Button
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
            <div className={forms.actions}>
              {!isDefault && (
                <Button disabled={busy} onClick={onDefault}>
                  {a.makeDefault}
                </Button>
              )}
              <Button
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
              <Button disabled={signOut.pending} onClick={() => void signOut.run()}>
                {a.launcherSignOut}
              </Button>
            </div>
          )}
          {!view.needsSignIn && (
            <>
              <div className={forms.actions}>
                <Button disabled={exporting} onClick={() => void exportData()}>
                  {a.launcherExport}
                </Button>
                <Button
                  disabled={view.signingIn || open.pending}
                  onClick={() => void open.run('delete')}
                >
                  {a.launcherDelete}
                </Button>
              </div>
              <p className={styles.hint}>{a.launcherDeleteHint}</p>
            </>
          )}
        </div>
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
