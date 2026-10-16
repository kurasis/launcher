import { Panel } from '../components/Panel';
import { StatusBadge } from '../components/StatusBadge';
import { MicrosoftSetupPanel } from '../features/accounts/MicrosoftSetupPanel';
import { SystemInfoPanel } from '../features/diagnostics/SystemInfoPanel';
import { strings } from '../lib/strings';
import styles from './App.module.css';
import { useAppInfo } from './useAppInfo';

export function App() {
  const { state, reload } = useAppInfo();

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <h1 className={styles.brand}>{strings.productName}</h1>
        <StatusBadge tone="neutral" label={strings.buildTag} />
      </header>

      <main className={styles.content}>
        <Panel title={strings.foundation.title}>
          <p>{strings.foundation.body}</p>
        </Panel>
        <SystemInfoPanel state={state} onRetry={reload} />
        {state.status === 'ready' && <MicrosoftSetupPanel status={state.info.microsoftClientId} />}
      </main>

      <footer className={styles.footer}>
        <p>{strings.independentNotice}</p>
      </footer>
    </div>
  );
}
