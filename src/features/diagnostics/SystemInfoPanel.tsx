import { Button } from '../../components/Button';
import { Panel } from '../../components/Panel';
import type { AppInfoState } from '../../app/useAppInfo';
import { strings } from '../../lib/strings';
import styles from './SystemInfoPanel.module.css';

interface SystemInfoPanelProps {
  state: AppInfoState;
  onRetry: () => void;
}

const t = strings.systemInfo;

/** Shows non-secret facts reported by the Rust core over IPC. */
export function SystemInfoPanel({ state, onRetry }: SystemInfoPanelProps) {
  return (
    <Panel title={t.title}>
      {state.status === 'loading' && (
        <p role="status" className={styles.muted}>
          {t.loading}
        </p>
      )}
      {state.status === 'error' && (
        <div role="alert" className={styles.error}>
          <p>
            {t.errorPrefix} {state.message}
          </p>
          <Button onClick={onRetry}>{t.retry}</Button>
        </div>
      )}
      {state.status === 'ready' && (
        <dl className={styles.facts}>
          <dt>{t.version}</dt>
          <dd>{state.info.appVersion}</dd>
          <dt>{t.platform}</dt>
          <dd>
            {state.info.os} / {state.info.arch}
          </dd>
          <dt>{t.build}</dt>
          <dd>{t.buildProfile[state.info.buildProfile]}</dd>
          <dt>{t.framework}</dt>
          <dd>{state.info.tauriVersion}</dd>
        </dl>
      )}
    </Panel>
  );
}
