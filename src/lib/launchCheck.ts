import type { ModsSetProblem } from './contracts';

/** A launch waiting on the player's answer about what the mods' files say. */
export interface LaunchQuestion {
  instanceId: string;
  problems: ModsSetProblem[];
  answer: (start: boolean) => void;
}

type Listener = (question: LaunchQuestion | null) => void;

const listeners = new Set<Listener>();
let current: LaunchQuestion | null = null;
/** The problems the player started with anyway, by installation, until the
 * launcher closes: the same ones are not asked about again. */
const accepted = new Map<string, string>();

function emit(): void {
  for (const listener of listeners) listener(current);
}

/** Hears each question (and `null` once it is answered). */
export function onLaunchQuestion(listener: Listener): () => void {
  listeners.add(listener);
  listener(current);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Whether to start an installation the mods' own files say may not start
 * (D-066): a warning the player answers, not a block. Without problems, or
 * with nothing on screen to ask with, it starts.
 */
export function askToStart(instanceId: string, problems: ModsSetProblem[]): Promise<boolean> {
  const key = JSON.stringify(problems);
  if (problems.length === 0 || listeners.size === 0 || accepted.get(instanceId) === key) {
    return Promise.resolve(true);
  }
  // One question at a time; an older one is taken as declined.
  current?.answer(false);
  return new Promise((resolve) => {
    const question: LaunchQuestion = {
      instanceId,
      problems,
      answer: (start) => {
        if (current !== question) return;
        if (start) accepted.set(instanceId, key);
        current = null;
        emit();
        resolve(start);
      },
    };
    current = question;
    emit();
  });
}
