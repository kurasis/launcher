import type {
  AccountSummary,
  InstanceSummary,
  MicrosoftAccount,
  ProfileRef,
} from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** Name of the built-in demo player when no profile is chosen (Rust `DEFAULT_NAME`). */
export const BUILT_IN_PLAYER = 'Player';

/** Who plays: a Microsoft account, or a demo player name. */
export type Player =
  { kind: 'microsoft'; account: MicrosoftAccount } | { kind: 'notoken'; name: string };

/** The account or profile `ref` names, if it still exists. */
export function lookup(account: AccountSummary, ref: ProfileRef | null): Player | null {
  if (ref === null) return null;
  if (ref.kind === 'microsoft') {
    const found = account.accounts.find((a) => a.profileId === ref.id);
    return found === undefined ? null : { kind: 'microsoft', account: found };
  }
  const found = account.notokenProfiles.find((p) => p.id === ref.id);
  return found === undefined ? null : { kind: 'notoken', name: found.name };
}

/** Who plays an installation: its own profile, the default, or the built-in player. */
export function playerFor(instance: InstanceSummary, account: AccountSummary): Player {
  return (
    lookup(account, instance.profile) ??
    lookup(account, account.defaultProfile) ?? { kind: 'notoken', name: BUILT_IN_PLAYER }
  );
}

/** The default player of new and unassigned installations. */
export function defaultPlayer(account: AccountSummary): Player {
  return lookup(account, account.defaultProfile) ?? { kind: 'notoken', name: BUILT_IN_PLAYER };
}

export function playerLabel(player: Player): string {
  return player.kind === 'microsoft'
    ? strings.accounts.microsoftLabel({ name: player.account.profileName })
    : strings.accounts.notokenLabel({ name: player.name });
}

export function sameProfile(a: ProfileRef | null, b: ProfileRef | null): boolean {
  return a?.kind === b?.kind && a?.id === b?.id;
}
