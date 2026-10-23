import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { SkinHead } from '../../components/SkinHead';
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
import { playModeHint, sameProfile } from './players';
import { ConfirmRemove } from './ConfirmRemove';
import { useMicrosoftHeads } from './useMicrosoftHeads';

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

/** Every page the Microsoft panel links to, at its foot. */
const allLinks: readonly ExternalLink[] = [
  'buy_minecraft',
  'create_profile',
  'xbox_profile',
  'family_settings',
  'minecraft_help',
  'minecraft_eula',
];

/** States in which checking the account again can tell something new. */
const checkable: ReadonlySet<AccountState> = new Set<AccountState>([
  'verified_access',
  'no_access',
  'profile_setup_required',
  'service_unavailable',
  'app_not_approved',
  'restricted',
  'protocol_error',
]);

/** Links to official pages, opened by the core. */
function Links({ links, label }: { links: readonly ExternalLink[]; label?: string }) {
  const opening = useShownError();
  if (links.length === 0) return null;
  return (
    <>
      <ul className={styles.links} aria-label={label}>
        {links.map((link) => (
          <li key={link}>
            <button
              type="button"
              className={styles.link}
              onClick={() => {
                opening.clear();
                void ipc.openExternal(link).catch(opening.catcher);
              }}
            >
              {t.links[link]}
            </button>
          </li>
        ))}
      </ul>
      {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
    </>
  );
}

interface RowProps {
  account: MicrosoftAccount;
  head: string | null;
  isDefault: boolean;
  busy: boolean;
  canSignIn: boolean;
  onSignIn: () => void;
  onDefault: () => void;
}

/** One Microsoft account: its head and state, sign in again, check again, default and
 * removal, and what helps with its state. */
function AccountRow({ account, head, isDefault, busy, canSignIn, onSignIn, onDefault }: RowProps) {
  const signOut = useCommand((requestId: string, id: string) => ipc.signOut(id, requestId));
  const check = useCommand((requestId: string, id: string) => ipc.verifyAccount(id, requestId));
  const state = account.state;
  const error = signOut.error ?? check.error;
  const disabled = busy || signOut.pending || check.pending;
  const signIn = state === 'reauth_required' && canSignIn;
  return (
    <li className={styles.item}>
      <SkinHead texture={null} head={head} size={40} className={styles.itemHead} />
      <div className={styles.itemTitle}>
        <strong dir="auto">{account.profileName}</strong>
        <StatusBadge tone={tones[state]} label={t.stateTitle[state]} />
        {isDefault && <span className={styles.tag}>{a.isDefault}</span>}
      </div>
      <div className={styles.itemActions}>
        {signIn && (
          <Button variant="primary" size="small" disabled={disabled} onClick={onSignIn}>
            {t.signInAgain}
          </Button>
        )}
        {checkable.has(state) && (
          <Button
            size="small"
            disabled={disabled}
            onClick={() => void check.run(account.profileId)}
          >
            {t.checkAgain}
          </Button>
        )}
        {!isDefault && (
          <Button size="small" disabled={disabled} onClick={onDefault}>
            {a.makeDefault}
          </Button>
        )}
        <ConfirmRemove
          question={a.confirmRemove({ name: account.profileName })}
          disabled={disabled}
          onRemove={() => void signOut.run(account.profileId)}
        />
      </div>
      {state === 'verified_access' && <p className={styles.itemMeta}>{t.verifiedNote}</p>}
      {signIn && <p className={styles.itemMeta}>{t.signInHint}</p>}
      {state === 'refreshing' && (
        <p className={styles.itemMeta} role="status">
          {t.checking}
        </p>
      )}
      {account.error !== null && state !== 'verified_access' && (
        <div className={styles.itemWide}>
          <Notice tone="warning">{localizeError(account.error)}</Notice>
        </div>
      )}
      {error !== null && (
        <div className={styles.itemWide}>
          <Notice tone="danger">{error}</Notice>
        </div>
      )}
      {(helpLinks[state]?.length ?? 0) > 0 && (
        <div className={styles.itemWide}>
          <Links links={helpLinks[state] ?? []} />
        </div>
      )}
    </li>
  );
}

/**
 * The Accounts screen in concept A (D-072): the launcher account and profiles without an
 * account in one column, Microsoft accounts (D-045) in the other. Add several of each,
 * remove them, and choose the default player of installations that name none.
 */
export function AccountView({
  snapshot,
  onOpenSkins,
}: {
  snapshot: AppSnapshot;
  onOpenSkins?: () => void;
}) {
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
  const heads = useMicrosoftHeads(account.accounts);
  const signingIn = account.loginOperationId !== null;
  const busy = signIn.pending || cancel.pending || setDefault.pending;
  const loginState = account.loginState;
  const isDefault = (profile: ProfileRef) => sameProfile(account.defaultProfile, profile);
  const modeLocked = setMode.pending || !snapshot.settings.writable;
  const notokenOffline = snapshot.settings.notokenOffline && account.fullGameAllowed;

  const own = account.launcherAccount;
  return (
    <Screen title={strings.nav.account}>
      <div className={styles.columns}>
        <div className={styles.column}>
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
                disabled={modeLocked}
                onChange={(offline) => void setMode.run({ launcherAccountOffline: offline })}
              />
            }
            modeHint={playModeHint(
              snapshot.settings.launcherAccountOffline && account.fullGameAllowed,
              account.fullGameAllowed,
            )}
            onOpenSkins={onOpenSkins}
          />
          <Panel
            title={a.notokenTitle}
            level={3}
            aside={
              <PlayModeSwitch
                offline={snapshot.settings.notokenOffline}
                allowed={account.fullGameAllowed}
                disabled={modeLocked}
                onChange={(offline) => void setMode.run({ notokenOffline: offline })}
              />
            }
          >
            <p className={styles.hint}>{a.notokenHint}</p>
            {/* The description already says what the demo is. */}
            {(notokenOffline || !account.fullGameAllowed) && (
              <p className={styles.hint}>{playModeHint(notokenOffline, account.fullGameAllowed)}</p>
            )}
            {account.notokenProfiles.length === 0 ? (
              <p className={styles.hint}>{a.notokenNone}</p>
            ) : (
              <ul className={styles.list}>
                {account.notokenProfiles.map((profile) => {
                  const ref: ProfileRef = { kind: 'notoken', id: profile.id };
                  return (
                    <li key={profile.id} className={[styles.item, styles.inline].join(' ')}>
                      <SkinHead texture={null} size={32} className={styles.itemHead} />
                      <div className={styles.itemTitle}>
                        <strong dir="ltr">{profile.name}</strong>
                        {isDefault(ref) && <span className={styles.tag}>{a.isDefault}</span>}
                      </div>
                      <p className={styles.itemMeta}>
                        {notokenOffline ? a.modeOffline : a.modeDemo}
                      </p>
                      <div className={styles.itemActions}>
                        {!isDefault(ref) && (
                          <Button
                            size="small"
                            disabled={busy}
                            onClick={() => void setDefault.run(ref)}
                          >
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
              className={styles.addForm}
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
                    className={[forms.input, styles.nameInput].join(' ')}
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
              <Button type="submit" disabled={addProfile.pending || name.trim() === ''}>
                {a.addProfile}
              </Button>
            </form>
            {(addProfile.error ?? removeProfile.error) !== null && (
              <Notice tone="danger">{addProfile.error ?? removeProfile.error}</Notice>
            )}
            {account.defaultProfile === null && <p className={styles.hint}>{a.defaultNone}</p>}
          </Panel>
        </div>

        <Panel title={a.microsoftTitle} level={3}>
          <p className={styles.hint}>{a.microsoftHint}</p>
          {account.accounts.length === 0 && !signingIn && <p className={styles.hint}>{a.none}</p>}
          {account.accounts.length > 0 && (
            <ul className={styles.list}>
              {account.accounts.map((item) => (
                <AccountRow
                  key={item.profileId}
                  account={item}
                  head={heads.get(item.profileId) ?? null}
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
                <Button
                  size="small"
                  disabled={retry.pending}
                  onClick={() => void retry.run(pending.profileId)}
                >
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
          {configured && (
            <div className={styles.actions}>
              {!signingIn && (
                <Button disabled={busy} onClick={() => void signIn.run()}>
                  <Icon name="plus" />
                  {a.add}
                </Button>
              )}
              {signingIn && account.loginOperationId !== null && (
                <Button
                  disabled={busy}
                  onClick={() => {
                    if (account.loginOperationId !== null)
                      void cancel.run(account.loginOperationId);
                  }}
                >
                  {t.cancel}
                </Button>
              )}
            </div>
          )}
          {configured && account.accounts.length === 0 && (
            <p className={styles.hint}>{t.signInHint}</p>
          )}
          {account.accounts.length > 0 && <p className={styles.hint}>{t.signOutHint}</p>}
          <Links links={allLinks} label={t.linksLabel} />
        </Panel>
      </div>
    </Screen>
  );
}
