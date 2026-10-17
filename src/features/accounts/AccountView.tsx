import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { AccountState, AppSnapshot, ExternalLink } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';

const t = strings.accountView;

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

export function AccountView({ snapshot }: { snapshot: AppSnapshot }) {
  const { account } = snapshot;
  const configured = snapshot.microsoftClientId === 'configured_unverified';
  const signIn = useCommand((requestId: string) => ipc.beginLogin(requestId));
  const cancel = useCommand((requestId: string, operationId: string) =>
    ipc.cancelLogin(operationId, requestId),
  );
  const signOut = useCommand((requestId: string) => ipc.signOut(requestId));
  const check = useCommand((requestId: string) => ipc.verifyAccount(requestId));
  const busy = signIn.pending || cancel.pending || signOut.pending || check.pending;
  const commandError = signIn.error ?? cancel.error ?? signOut.error ?? check.error;

  const state = account.state;
  const canSignIn = configured && ['signed_out', 'reauth_required'].includes(state);
  const signedIn = account.profileName !== null && state !== 'signing_in';

  return (
    <Panel title={t.title} aside={<StatusBadge tone={tones[state]} label={t.stateTitle[state]} />}>
      {account.profileName !== null && (
        <p>
          <strong>{account.profileName}</strong>
        </p>
      )}
      {state === 'verified_access' && (
        <>
          <p>{t.verified}</p>
          <p>{t.verifiedNote}</p>
        </>
      )}
      {state === 'signing_in' && <p role="status">{t.waiting}</p>}
      {state === 'refreshing' && <p role="status">{t.checking}</p>}
      {!configured && state === 'signed_out' && <Notice tone="info">{t.notConfigured}</Notice>}
      {account.error !== null && state !== 'verified_access' && (
        <Notice tone={state === 'signed_out' ? 'info' : 'warning'}>{account.error.message}</Notice>
      )}
      {account.sessionOnly && <Notice tone="info">{t.sessionOnly}</Notice>}
      {commandError !== null && <Notice tone="danger">{commandError}</Notice>}

      <div className={forms.actions}>
        {canSignIn && (
          <Button
            variant="primary"
            disabled={busy}
            onClick={() => {
              void signIn.run();
            }}
          >
            {state === 'reauth_required' ? t.signInAgain : t.signIn}
          </Button>
        )}
        {state === 'signing_in' && account.loginOperationId !== null && (
          <Button
            disabled={busy}
            onClick={() => {
              if (account.loginOperationId !== null) void cancel.run(account.loginOperationId);
            }}
          >
            {t.cancel}
          </Button>
        )}
        {(state === 'service_unavailable' || state === 'protocol_error') && signedIn && (
          <Button
            disabled={busy}
            onClick={() => {
              void check.run();
            }}
          >
            {t.checkAgain}
          </Button>
        )}
        {(signedIn || state === 'sign_out_pending') && (
          <Button
            disabled={busy}
            onClick={() => {
              void signOut.run();
            }}
          >
            {state === 'sign_out_pending' ? t.retrySignOut : t.signOut}
          </Button>
        )}
        {(helpLinks[state] ?? []).map((link) => (
          <Button
            key={link}
            onClick={() => {
              void ipc.openExternal(link);
            }}
          >
            {t.links[link]}
          </Button>
        ))}
      </div>
      {canSignIn && <p>{t.signInHint}</p>}
      {signedIn && <p>{t.signOutHint}</p>}
    </Panel>
  );
}
