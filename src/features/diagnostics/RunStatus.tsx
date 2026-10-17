import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import type { RunState, RunSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './RunStatus.module.css';
import { localizeError } from '../../lib/strings';

const t = strings.runs;

const tones: Record<RunState, StatusTone> = {
  preflight: 'neutral',
  spawning: 'neutral',
  running: 'success',
  stopping: 'warning',
  exited: 'success',
  failed: 'danger',
  cancelled: 'neutral',
  stopped: 'neutral',
};

interface RunStatusProps {
  current: RunSummary | null;
  last: RunSummary | null;
  onStop?: (runId: string) => void;
}

/** Current run and the recorded result of the last one. */
export function RunStatus({ current, last, onStop }: RunStatusProps) {
  const [confirming, setConfirming] = useState<string | null>(null);
  if (current !== null) {
    const isGame = current.kind === 'game';
    const asking = isGame && confirming === current.runId;
    return (
      <Panel
        title={`${t.currentTitle}: ${t.kind[current.kind]}`}
        aside={<StatusBadge tone={tones[current.state]} label={t.state[current.state]} />}
      >
        {onStop !== undefined && current.state === 'running' && !asking && (
          <div>
            <Button
              onClick={() => {
                if (isGame) setConfirming(current.runId);
                else onStop(current.runId);
              }}
            >
              {isGame ? t.stopGame : t.stop}
            </Button>
          </div>
        )}
        {onStop !== undefined && asking && current.state === 'running' && (
          <Notice tone="warning">
            <p>{t.stopWarning}</p>
            <div className={styles.actions}>
              <Button
                onClick={() => {
                  setConfirming(null);
                  onStop(current.runId);
                }}
              >
                {t.stopConfirm}
              </Button>
              <Button
                variant="primary"
                onClick={() => {
                  setConfirming(null);
                }}
              >
                {t.keepPlaying}
              </Button>
            </div>
          </Notice>
        )}
      </Panel>
    );
  }
  if (last === null) return null;
  // A run that failed in preflight never had a process.
  const notStarted = last.state === 'failed' && last.durationMs === null;
  return (
    <Panel
      title={`${t.lastTitle}: ${t.kind[last.kind]}`}
      aside={
        <StatusBadge
          tone={tones[last.state]}
          label={notStarted ? t.notStarted : t.state[last.state]}
        />
      }
    >
      <dl className={styles.facts}>
        <dt>{t.exitCode}</dt>
        <dd>{last.exitCode ?? '—'}</dd>
        <dt>{t.duration}</dt>
        <dd>{last.durationMs === null ? '—' : t.seconds({ seconds: last.durationMs / 1000 })}</dd>
      </dl>
      {last.error !== null && <Notice tone="danger">{localizeError(last.error)}</Notice>}
      {last.drainTimedOut && <Notice tone="info">{t.drainTimedOut}</Notice>}
      {last.loggingDegraded && <Notice tone="warning">{t.loggingDegraded}</Notice>}
      {last.outputTail.length > 0 && (
        <details>
          <summary>{t.outputTitle}</summary>
          {last.outputLinesDropped > 0 && (
            <p className={styles.muted}>{t.dropped({ count: last.outputLinesDropped })}</p>
          )}
          <pre className={styles.output}>{last.outputTail.join('\n')}</pre>
        </details>
      )}
    </Panel>
  );
}
