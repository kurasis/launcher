// The one main action of an installation: what the launch bar offers for it, and
// what Ctrl+Enter runs in the command palette. Pure: the caller runs the command.
import type { AppSnapshot, InstanceSummary } from '../../lib/contracts';
import { progressPercent } from '../../lib/format';
import { strings } from '../../lib/strings';

export type MainActionKind =
  | 'play'
  | 'install'
  | 'retry'
  | 'repair'
  /** Install or repair while something else runs or the internet is away (D-049). */
  | 'queue'
  /** The installation's game is running. */
  | 'running'
  /** The installation's own install, repair or other operation is in progress. */
  | 'busy'
  /** Nothing to do: an installation made by a newer launcher. */
  | 'none';

/** The command a main action runs; null when it is a status, not a button. */
export type MainActionCommand = 'launch' | 'install' | 'repair' | null;

export interface MainAction {
  kind: MainActionKind;
  /** The button's text, or the status shown in its place; empty for `none`. */
  label: string;
  /** Whether pressing it does something now. */
  enabled: boolean;
  command: MainActionCommand;
  /** Its install or repair already waits in the queue. */
  queued: boolean;
}

/** Commands of this installation already sent and not answered yet. */
export interface PendingCommands {
  play: boolean;
  install: boolean;
  repair: boolean;
}

const NOTHING_PENDING: PendingCommands = { play: false, install: false, repair: false };

/**
 * The main action of `instance` now: Play when it is installed, Install (Retry
 * install after an unfinished one) when it is not, Repair otherwise; Add to queue
 * while another operation runs or the internet is away; the running game or its
 * own progress as a status.
 */
export function mainActionOf(
  instance: InstanceSummary,
  snapshot: AppSnapshot,
  pending: PendingCommands = NOTHING_PENDING,
): MainAction {
  const t = strings.install;
  const active = snapshot.activeOperation;
  const mine = active?.instanceId === instance.id ? active : null;
  const otherBusy = active !== null && mine === null;
  const offline = snapshot.network === 'offline';
  const queued = snapshot.queue.some((q) => q.instanceId === instance.id);
  const running = mine?.kind === 'launch' || snapshot.currentRun?.instanceId === instance.id;
  const health = instance.health;

  if (running) {
    return { kind: 'running', label: t.gameRunning, enabled: false, command: null, queued };
  }
  if (mine !== null) {
    const progress =
      snapshot.operationProgress?.operationId === mine.operationId
        ? snapshot.operationProgress
        : null;
    const percent = progressPercent(progress);
    const label =
      percent !== null
        ? `${strings.activity[mine.kind]} ${strings.activity.percent({ percent })}`
        : strings.activity[mine.kind];
    return { kind: 'busy', label, enabled: false, command: null, queued };
  }
  if (health === 'ready') {
    return {
      kind: 'play',
      label: pending.play ? t.starting : t.play,
      enabled: !otherBusy && !pending.play,
      command: 'launch',
      queued,
    };
  }
  const waits = otherBusy || offline;
  if (health === 'not_installed') {
    const last = snapshot.recentOperations.find((r) => r.instanceId === instance.id) ?? null;
    const unfinished = last !== null && last.kind === 'install' && last.outcome !== 'succeeded';
    return {
      kind: waits ? 'queue' : unfinished ? 'retry' : 'install',
      label: waits ? strings.downloads.addToQueue : unfinished ? t.retry : t.install,
      enabled: !pending.install && !queued,
      command: 'install',
      queued,
    };
  }
  if (health !== 'unsupported') {
    return {
      kind: waits ? 'queue' : 'repair',
      label: waits ? strings.downloads.addToQueue : t.repair,
      enabled: !pending.repair && !queued,
      command: 'repair',
      queued,
    };
  }
  return { kind: 'none', label: '', enabled: false, command: null, queued };
}
