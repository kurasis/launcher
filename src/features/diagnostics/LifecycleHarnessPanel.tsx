import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, LifecycleScenario } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { RunStatus } from './RunStatus';

const t = strings.harness;
const scenarios: LifecycleScenario[] = [
  'short',
  'long',
  'immediate_exit',
  'crash',
  'inherited_pipe',
  'noisy',
];

/** Debug-only controls for the P03 lifecycle proof. */
export function LifecycleHarnessPanel({ snapshot }: { snapshot: AppSnapshot }) {
  const start = useCommand((requestId: string, scenario: LifecycleScenario) =>
    ipc.startLifecycleTest(scenario, requestId),
  );
  const busy = snapshot.activeOperation !== null || start.pending;
  return (
    <>
      <Panel title={t.title}>
        <p>{t.explanation}</p>
        <div className={forms.actions}>
          {scenarios.map((scenario) => (
            <Button
              key={scenario}
              disabled={busy}
              onClick={() => {
                void start.run(scenario);
              }}
            >
              {t.scenarios[scenario]}
            </Button>
          ))}
        </div>
        {start.error !== null && <Notice tone="danger">{start.error}</Notice>}
      </Panel>
      <RunStatus
        current={snapshot.currentRun}
        last={snapshot.lastRun}
        onStop={(runId) => {
          void ipc.stopLifecycleTest(runId).catch(() => undefined);
        }}
      />
    </>
  );
}
