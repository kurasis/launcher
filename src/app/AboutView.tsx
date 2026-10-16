import { Panel } from '../components/Panel';
import { MicrosoftSetupPanel } from '../features/accounts/MicrosoftSetupPanel';
import { SystemInfoPanel } from '../features/diagnostics/SystemInfoPanel';
import { strings } from '../lib/strings';
import { useAppInfo } from './useAppInfo';

export function AboutView() {
  const { state, reload } = useAppInfo();
  return (
    <>
      <Panel title={strings.foundation.title}>
        <p>{strings.foundation.body}</p>
        <p>{strings.independentNotice}</p>
      </Panel>
      <SystemInfoPanel state={state} onRetry={reload} />
      {state.status === 'ready' && <MicrosoftSetupPanel status={state.info.microsoftClientId} />}
    </>
  );
}
