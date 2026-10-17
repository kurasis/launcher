import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import type { RunState, RunSummary } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import styles from './RunStatus.module.css';

const t = strings.runs;

const tones: Record<RunState, StatusTone> = {
  preflight: 'neutral',
  spawning: 'neutral',
  running: 'success',
  stopping: 'warning',
  exited: 'success',
  failed: 'danger',
  cancelled: 'neutral',
};

interface RunStatusProps {
  current: RunSummary | null;
  last: RunSummary | null;
  onStop?: (runId: string) => void;
}

/** Current run and the recorded result of the last one. */
export function RunStatus({ current, last, onStop }: RunStatusProps) {
  if (current !== null) {
    return (
      <Panel
        title={`${t.currentTitle}: ${t.kind[current.kind]}`}
        aside={<StatusBadge tone={tones[current.state]} label={t.state[current.state]} />}
      >
        {onStop !== undefined && current.state === 'running' && (
          <div>
            <Button
              onClick={() => {
                onStop(current.runId);
              }}
            >
              {t.stop}
            </Button>
          </div>
        )}
      </Panel>
    );
  }
  if (last === null) return null;
  return (
    <Panel
      title={`${t.lastTitle}: ${t.kind[last.kind]}`}
      aside={<StatusBadge tone={tones[last.state]} label={t.state[last.state]} />}
    >
      <dl className={styles.facts}>
        <dt>{t.exitCode}</dt>
        <dd>{last.exitCode ?? '—'}</dd>
        <dt>{t.duration}</dt>
        <dd>{last.durationMs === null ? '—' : t.seconds(last.durationMs)}</dd>
      </dl>
      {last.error !== null && <Notice tone="danger">{last.error.message}</Notice>}
      {last.drainTimedOut && <Notice tone="info">{t.drainTimedOut}</Notice>}
      {last.outputTail.length > 0 && (
        <details>
          <summary>{t.outputTitle}</summary>
          {last.outputLinesDropped > 0 && (
            <p className={styles.muted}>{t.dropped(last.outputLinesDropped)}</p>
          )}
          <pre className={styles.output}>{last.outputTail.join('\n')}</pre>
        </details>
      )}
    </Panel>
  );
}
