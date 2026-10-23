import type { AccountSummary, MicrosoftAccount, ProfileRef } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** Name of the built-in demo player when no profile is chosen (Rust `DEFAULT_NAME`). */
export const BUILT_IN_PLAYER = 'Player';

/** Who plays: a Microsoft account, a demo player name, or the launcher account. */
export type Player =
  | { kind: 'microsoft'; account: MicrosoftAccount }
  | { kind: 'notoken'; name: string }
  | { kind: 'launcher'; name: string };

/** The account or profile `ref` names, if it still exists. */
export function lookup(account: AccountSummary, ref: ProfileRef | null): Player | null {
  if (ref === null) return null;
  if (ref.kind === 'microsoft') {
    const found = account.accounts.find((a) => a.profileId === ref.id);
    return found === undefined ? null : { kind: 'microsoft', account: found };
  }
  if (ref.kind === 'launcher') {
    const own = account.launcherAccount;
    return own.id === ref.id && own.nick !== null ? { kind: 'launcher', name: own.nick } : null;
  }
  const found = account.notokenProfiles.find((p) => p.id === ref.id);
  return found === undefined ? null : { kind: 'notoken', name: found.name };
}

/** The default player of new and unassigned installations. */
export function defaultPlayer(account: AccountSummary): Player {
  return lookup(account, account.defaultProfile) ?? { kind: 'notoken', name: BUILT_IN_PLAYER };
}

export function playerLabel(player: Player): string {
  switch (player.kind) {
    case 'microsoft':
      return strings.accounts.microsoftLabel({ name: player.account.profileName });
    case 'launcher':
      return strings.accounts.launcherLabel({ name: player.name });
    case 'notoken':
      return strings.accounts.notokenLabel({ name: player.name });
  }
}

export function sameProfile(a: ProfileRef | null, b: ProfileRef | null): boolean {
  return a?.kind === b?.kind && a?.id === b?.id;
}

/** What the chosen play mode means, or why offline cannot be chosen. */
export function playModeHint(offline: boolean, allowed: boolean): string {
  const a = strings.accounts;
  return !allowed ? a.offlineNeedsOwner : offline ? a.offlineHint : a.demoHint;
}
