import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  AccountState,
  AppSnapshot,
  ExternalLink,
  MicrosoftAccount,
  ProfileRef,
  SettingsPatch,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { useShownError } from '../../lib/useShownError';
import { localizeError, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Accounts.module.css';
import { LauncherAccountPanel } from './LauncherAccountPanel';
import { PlayModeSwitch } from './PlayModeSwitch';
import { sameProfile } from './players';
import { ConfirmRemove } from './ConfirmRemove';

const t = strings.accountView;
const a = strings.accounts;

const tones: Record<AccountState, StatusTone> = {
  signed_out: 'neutral',
  signing_in: 'neutral',
  refreshing: 'neutral',
  verified_access: 'success',
  no_access: 'warning',
  profile_setup_required: 'warning',
  reauth_required: 'warning',
  service_unavailable: 'warning',
  app_not_approved: 'danger',
  restricted: 'danger',
  protocol_error: 'danger',
  sign_out_pending: 'danger',
};

/** Official pages that help with a given state. Opened by Rust via an allowlist. */
const helpLinks: Partial<Record<AccountState, ExternalLink[]>> = {
  no_access: ['buy_minecraft', 'minecraft_help'],
  profile_setup_required: ['create_profile', 'xbox_profile'],
  restricted: ['family_settings', 'xbox_profile', 'minecraft_help'],
  app_not_approved: ['minecraft_help'],
};

function HelpLinks({ state }: { state: AccountState }) {
  const opening = useShownError();
  return (
    <>
      {(helpLinks[state] ?? []).map((link) => (
        <Button
          key={link}
          onClick={() => {
            opening.clear();
            void ipc.openExternal(link).catch(opening.catcher);
          }}
        >
          {t.links[link]}
        </Button>
      ))}
      {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
    </>
  );
}

interface RowProps {
  account: MicrosoftAccount;
  isDefault: boolean;
  busy: boolean;
  canSignIn: boolean;
  onSignIn: () => void;
  onDefault: () => void;
}

/** One Microsoft account: its state, help, check again, default and removal. */
function AccountRow({ account, isDefault, busy, canSignIn, onSignIn, onDefault }: RowProps) {
  const signOut = useCommand((requestId: string, id: string) => ipc.signOut(id, requestId));
  const check = useCommand((requestId: string, id: string) => ipc.verifyAccount(id, requestId));
  const state = account.state;
  const error = signOut.error ?? check.error;
  const disabled = busy || signOut.pending || check.pending;
  return (
    <li className={styles.row}>
      <div className={styles.rowHead}>
        <strong>{account.profileName}</strong>
        {isDefault && <span className={styles.tag}>{a.isDefault}</span>}
        <StatusBadge tone={tones[state]} label={t.stateTitle[state]} />
      </div>
      {state === 'verified_access' && <p className={styles.hint}>{t.verifiedNote}</p>}
      {state === 'refreshing' && <p role="status">{t.checking}</p>}
      {account.error !== null && state !== 'verified_access' && (
        <Notice tone="warning">{localizeError(account.error)}</Notice>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={forms.actions}>
        {!isDefault && (
          <Button disabled={disabled} onClick={onDefault}>
            {a.makeDefault}
          </Button>
        )}
        {state === 'reauth_required' && canSignIn && (
          <Button variant="primary" disabled={disabled} onClick={onSignIn}>
            {t.signInAgain}
          </Button>
        )}
        {(state === 'service_unavailable' || state === 'protocol_error') && (
          <Button disabled={disabled} onClick={() => void check.run(account.profileId)}>
            {t.checkAgain}
          </Button>
        )}
        <ConfirmRemove
          question={a.confirmRemove({ name: account.profileName })}
          disabled={disabled}
          onRemove={() => void signOut.run(account.profileId)}
        />
        <HelpLinks state={state} />
      </div>
    </li>
  );
}

/**
 * Microsoft accounts and no-token profiles (D-045): add several of each, remove
 * them, and choose the default player of installations that name none.
 */
export function AccountView({ snapshot }: { snapshot: AppSnapshot }) {
  const { account } = snapshot;
  const configured = snapshot.microsoftClientId === 'configured_unverified';
  const [name, setName] = useState('');
  const signIn = useCommand((requestId: string) => ipc.beginLogin(requestId));
  const cancel = useCommand((requestId: string, operationId: string) =>
    ipc.cancelLogin(operationId, requestId),
  );
  const retry = useCommand((requestId: string, id: string) => ipc.signOut(id, requestId));
  const setDefault = useCommand((requestId: string, profile: ProfileRef | null) =>
    ipc.setDefaultProfile(profile, requestId),
  );
  const addProfile = useCommand((requestId: string, profileName: string) =>
    ipc.addNoTokenProfile(profileName, requestId),
  );
  const removeProfile = useCommand((requestId: string, id: string) =>
    ipc.removeNoTokenProfile(id, requestId),
  );
  const setMode = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const signingIn = account.loginOperationId !== null;
  const busy = signIn.pending || cancel.pending || setDefault.pending;
  const loginState = account.loginState;
  const isDefault = (profile: ProfileRef) => sameProfile(account.defaultProfile, profile);

  const own = account.launcherAccount;
  return (
    <>
      <LauncherAccountPanel
        view={own}
        isDefault={own.id !== null && isDefault({ kind: 'launcher', id: own.id })}
        busy={busy}
        onDefault={() => {
          if (own.id !== null) void setDefault.run({ kind: 'launcher', id: own.id });
        }}
        mode={
          <PlayModeSwitch
            offline={snapshot.settings.launcherAccountOffline}
            allowed={account.fullGameAllowed}
            disabled={setMode.pending || !snapshot.settings.writable}
            onChange={(offline) => void setMode.run({ launcherAccountOffline: offline })}
          />
        }
      />
      <Panel title={a.notokenTitle}>
        <p className={styles.hint}>{a.notokenHint}</p>
        <PlayModeSwitch
          offline={snapshot.settings.notokenOffline}
          allowed={account.fullGameAllowed}
          disabled={setMode.pending || !snapshot.settings.writable}
          onChange={(offline) => void setMode.run({ notokenOffline: offline })}
        />
        {account.notokenProfiles.length === 0 ? (
          <p>{a.notokenNone}</p>
        ) : (
          <ul className={styles.list}>
            {account.notokenProfiles.map((profile) => {
              const ref: ProfileRef = { kind: 'notoken', id: profile.id };
              return (
                <li key={profile.id} className={styles.row}>
                  <div className={styles.rowHead}>
                    <strong>{profile.name}</strong>
                    {isDefault(ref) && <span className={styles.tag}>{a.isDefault}</span>}
                  </div>
                  <div className={forms.actions}>
                    {!isDefault(ref) && (
                      <Button disabled={busy} onClick={() => void setDefault.run(ref)}>
                        {a.makeDefault}
                      </Button>
                    )}
                    <ConfirmRemove
                      question={a.confirmRemoveProfile({ name: profile.name })}
                      disabled={removeProfile.pending}
                      onRemove={() => void removeProfile.run(profile.id)}
                    />
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        <form
          className={forms.form}
          aria-label={a.addProfile}
          onSubmit={(event) => {
            event.preventDefault();
            void addProfile.run(name.trim()).then((added) => {
              if (added) setName('');
            });
          }}
        >
          <Field label={a.nameLabel} hint={a.nameHint}>
            {(props) => (
              <input
                {...props}
                className={forms.input}
                dir="ltr"
                maxLength={16}
                spellCheck={false}
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                  addProfile.reset();
                }}
              />
            )}
          </Field>
          {(addProfile.error ?? removeProfile.error) !== null && (
            <Notice tone="danger">{addProfile.error ?? removeProfile.error}</Notice>
          )}
          <div className={forms.actions}>
            <Button type="submit" disabled={addProfile.pending || name.trim() === ''}>
              {a.addProfile}
            </Button>
          </div>
        </form>
        {account.defaultProfile === null && <p className={styles.hint}>{a.defaultNone}</p>}
      </Panel>

      <Panel title={a.microsoftTitle}>
        <p className={styles.hint}>{a.microsoftHint}</p>
        {account.accounts.length === 0 && !signingIn && <p>{a.none}</p>}
        {account.accounts.length > 0 && (
          <ul className={styles.list}>
            {account.accounts.map((item) => (
              <AccountRow
                key={item.profileId}
                account={item}
                isDefault={isDefault({ kind: 'microsoft', id: item.profileId })}
                busy={busy}
                canSignIn={configured && !signingIn}
                onSignIn={() => void signIn.run()}
                onDefault={() => void setDefault.run({ kind: 'microsoft', id: item.profileId })}
              />
            ))}
          </ul>
        )}
        {account.pendingSignOuts.map((pending) => (
          <Notice key={pending.profileId} tone="danger">
            <p>{a.pendingSignOut({ name: pending.profileName })}</p>
            <div className={forms.actions}>
              <Button disabled={retry.pending} onClick={() => void retry.run(pending.profileId)}>
                {a.retryRemove}
              </Button>
            </div>
          </Notice>
        ))}
        {signingIn && <p role="status">{t.waiting}</p>}
        {!configured && <Notice tone="info">{t.notConfigured}</Notice>}
        {account.loginError !== null && !signingIn && (
          <Notice tone={loginState === 'signed_out' ? 'info' : 'warning'}>
            {localizeError(account.loginError)}
          </Notice>
        )}
        {account.sessionOnly && <Notice tone="info">{t.sessionOnly}</Notice>}
        {(signIn.error ?? cancel.error ?? retry.error ?? setDefault.error) !== null && (
          <Notice tone="danger">
            {signIn.error ?? cancel.error ?? retry.error ?? setDefault.error}
          </Notice>
        )}
        <div className={forms.actions}>
          {configured && !signingIn && (
            <Button variant="primary" disabled={busy} onClick={() => void signIn.run()}>
              {a.add}
            </Button>
          )}
          {signingIn && account.loginOperationId !== null && (
            <Button
              disabled={busy}
              onClick={() => {
                if (account.loginOperationId !== null) void cancel.run(account.loginOperationId);
              }}
            >
              {t.cancel}
            </Button>
          )}
          {!signingIn && <HelpLinks state={loginState} />}
        </div>
        {configured && <p className={styles.hint}>{t.signInHint}</p>}
        {account.accounts.length > 0 && <p className={styles.hint}>{t.signOutHint}</p>}
      </Panel>
    </>
  );
}
