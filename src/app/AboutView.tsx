import { Panel } from '../components/Panel';
import { MicrosoftSetupPanel } from '../features/accounts/MicrosoftSetupPanel';
import { LifecycleHarnessPanel } from '../features/diagnostics/LifecycleHarnessPanel';
import { SystemInfoPanel } from '../features/diagnostics/SystemInfoPanel';
import type { AppSnapshot } from '../lib/contracts';
import { strings } from '../lib/strings';
import { useAppInfo } from './useAppInfo';

export function AboutView({ snapshot }: { snapshot: AppSnapshot | null }) {
  const { state, reload } = useAppInfo();
  return (
    <>
      {snapshot?.lifecycleHarnessAvailable === true && (
        <LifecycleHarnessPanel snapshot={snapshot} />
      )}
      <Panel title={strings.foundation.title}>
        <p>{strings.foundation.body}</p>
        <p>{strings.independentNotice}</p>
      </Panel>
      <SystemInfoPanel state={state} onRetry={reload} />
      {state.status === 'ready' && <MicrosoftSetupPanel status={state.info.microsoftClientId} />}
    </>
  );
}
