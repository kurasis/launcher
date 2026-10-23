import { useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Fact, Facts } from '../../components/Facts';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StateDot, type DotTone } from '../../components/StateDot';
import { useFocusBack } from '../../components/useFocusBack';
import type { RunState, RunSummary } from '../../lib/contracts';
import { localizeError, strings } from '../../lib/strings';
import { isolate } from '../mods/helpers';
import { crashTitle } from './crashText';
import styles from './RunStatus.module.css';

const t = strings.runs;

const tones: Record<RunState, DotTone> = {
  preflight: 'busy',
  spawning: 'busy',
  running: 'busy',
  stopping: 'warning',
  exited: 'success',
  failed: 'danger',
  cancelled: 'neutral',
  stopped: 'neutral',
};

interface RunStatusProps {
  current: RunSummary | null;
  last: RunSummary | null;
  /** Asks the run to stop; a stop that fails offers Stop again. */
  onStop?: (runId: string) => Promise<unknown>;
  /** The panels' heading level: 3 under a screen's title. */
  level?: 2 | 3;
  /** Installation names by id: What names the installation the game ran from. */
  names?: ReadonlyMap<string, string>;
  /** The last output lines in view (Diagnostics), or folded under their title. */
  output?: 'shown' | 'folded';
}

/** "Minecraft, “Survival”", or the kind alone when the installation is not known. The
 * name keeps its own direction. */
function what(run: RunSummary, names: ReadonlyMap<string, string> | undefined): string {
  const name = run.instanceId === null ? undefined : names?.get(run.instanceId);
  return name === undefined
    ? t.kind[run.kind]
    : t.whatInstance({ kind: t.kind[run.kind], name: isolate(name) });
}

/** "4.2 s" under a minute, "42 min 10 s" above. */
function duration(ms: number): string {
  if (ms < 60_000) return t.seconds({ seconds: ms / 1000 });
  const total = Math.round(ms / 1000);
  return t.minutesSeconds({ minutes: Math.floor(total / 60), seconds: total % 60 });
}

/** Current run and the recorded result of the last one. */
export function RunStatus({
  current,
  last,
  onStop,
  level = 2,
  names,
  output = 'folded',
}: RunStatusProps) {
  const [confirming, setConfirming] = useState<string | null>(null);
  // The run asked to stop: Stop is not offered again while it stops.
  const [stopSent, setStopSent] = useState<string | null>(null);
  const stopButton = useRef<HTMLButtonElement>(null);
  const currentFacts = useRef<HTMLDivElement>(null);
  const lastFacts = useRef<HTMLDivElement>(null);
  const asking = current !== null && current.kind === 'game' && confirming === current.runId;
  const sent = current !== null && stopSent === current.runId;
  // The question replaces Stop game and gives the focus back to it when it closes; when
  // Stop goes (the run stops, or ends) the focus goes to what the run is now.
  useFocusBack(
    `${String(asking)}:${String(sent)}:${current?.runId ?? ''}:${current?.state ?? ''}`,
    stopButton,
    currentFacts,
    lastFacts,
  );
  const stop = (runId: string) => {
    if (onStop === undefined) return;
    setStopSent(runId);
    onStop(runId).catch(() => {
      setStopSent((id) => (id === runId ? null : id));
    });
  };
  if (current !== null) {
    const isGame = current.kind === 'game';
    return (
      <Panel title={`${t.currentTitle}: ${t.kind[current.kind]}`} level={level}>
        <div ref={currentFacts} className={styles.facts} tabIndex={-1}>
          <Facts>
            <Fact term={t.what}>{what(current, names)}</Fact>
            <Fact term={t.stateLabel}>
              <StateDot tone={tones[current.state]} label={t.state[current.state]} />
            </Fact>
          </Facts>
        </div>
        {onStop !== undefined && current.state === 'running' && !asking && !sent && (
          <div className={styles.actions}>
            <Button
              ref={stopButton}
              variant="danger"
              size="small"
              onClick={() => {
                if (isGame) setConfirming(current.runId);
                else stop(current.runId);
              }}
            >
              {isGame ? t.stopGame : t.stop}
            </Button>
          </div>
        )}
        {onStop !== undefined && asking && current.state === 'running' && (
          <Confirm
            label={t.stopGame}
            text={t.stopWarning}
            action={t.stopConfirm}
            cancel={t.keepPlaying}
            pending={false}
            onYes={() => {
              setConfirming(null);
              stop(current.runId);
            }}
            onNo={() => {
              setConfirming(null);
            }}
          />
        )}
      </Panel>
    );
  }
  if (last === null) return null;
  // A run that failed in preflight never had a process.
  const notStarted = last.state === 'failed' && last.durationMs === null;
  const lines = last.outputTail.join('\n');
  return (
    <Panel title={t.lastTitle} level={level}>
      <div ref={lastFacts} className={styles.facts} tabIndex={-1}>
        <Facts>
          <Fact term={t.what}>{what(last, names)}</Fact>
          <Fact term={t.stateLabel}>
            <StateDot
              tone={tones[last.state]}
              label={notStarted ? t.notStarted : t.state[last.state]}
            />
          </Fact>
          <Fact term={t.exitCode} dir="ltr">
            {last.exitCode ?? (last.signal !== null ? `SIG ${String(last.signal)}` : '—')}
          </Fact>
          <Fact term={t.duration}>
            {last.durationMs === null ? '—' : duration(last.durationMs)}
          </Fact>
        </Facts>
      </div>
      {last.diagnosis !== null ? (
        <Notice tone="danger">
          {strings.crash.summary({
            title: crashTitle(last.diagnosis.kind, last.diagnosis.details),
          })}
        </Notice>
      ) : (
        last.error !== null && <Notice tone="danger">{localizeError(last.error)}</Notice>
      )}
      {last.drainTimedOut && <Notice tone="info">{t.drainTimedOut}</Notice>}
      {last.loggingDegraded && <Notice tone="warning">{t.loggingDegraded}</Notice>}
      {last.outputTail.length > 0 &&
        (output === 'shown' ? (
          <div className={styles.block}>
            <p className={styles.muted}>{t.outputTitle}</p>
            {last.outputLinesDropped > 0 && (
              <p className={styles.muted}>{t.dropped({ count: last.outputLinesDropped })}</p>
            )}
            {/* The game's own words: text only, scrolling from the keyboard too. */}
            <pre
              className={styles.output}
              dir="ltr"
              tabIndex={0}
              role="region"
              aria-label={t.outputTitle}
            >
              {lines}
            </pre>
          </div>
        ) : (
          <details className={styles.folded}>
            <summary>{t.outputTitle}</summary>
            {last.outputLinesDropped > 0 && (
              <p className={styles.muted}>{t.dropped({ count: last.outputLinesDropped })}</p>
            )}
            <pre className={styles.output} dir="ltr">
              {lines}
            </pre>
          </details>
        ))}
    </Panel>
  );
}
