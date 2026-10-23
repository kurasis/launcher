import { Button } from '../../components/Button';
import { Fact, Facts } from '../../components/Facts';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import type { AppInfoState } from '../../app/useAppInfo';
import { formatBytes } from '../../lib/format';
import { strings } from '../../lib/strings';
import styles from './SystemInfoPanel.module.css';

interface SystemInfoPanelProps {
  state: AppInfoState;
  onRetry: () => void;
  /** The heading's level: 3 inside a screen's title. */
  level?: 2 | 3;
  /** This computer's memory, on Diagnostics; About shows the build facts only. */
  memoryMib?: number | null;
}

const t = strings.systemInfo;
const MIB = 1024 * 1024;

/** Shows non-secret facts reported by the Rust core over IPC. */
export function SystemInfoPanel({ state, onRetry, level = 2, memoryMib }: SystemInfoPanelProps) {
  return (
    <Panel title={t.title} level={level}>
      {state.status === 'loading' && (
        <p role="status" className={styles.muted}>
          {t.loading}
        </p>
      )}
      {state.status === 'error' && (
        <Notice tone="danger">
          <div className={styles.error}>
            <p>
              {t.errorPrefix} {state.message}
            </p>
            <Button size="small" onClick={onRetry}>
              {t.retry}
            </Button>
          </div>
        </Notice>
      )}
      {state.status === 'ready' && (
        <Facts>
          <Fact term={t.version}>
            <bdi>{state.info.appVersion}</bdi>
          </Fact>
          {/* "linux / x86_64": one value, read left to right. */}
          <Fact term={t.platform} dir="ltr">
            {state.info.os} / {state.info.arch}
          </Fact>
          <Fact term={t.build}>{t.buildProfile[state.info.buildProfile]}</Fact>
          <Fact term={t.framework}>
            <bdi>{state.info.tauriVersion}</bdi>
          </Fact>
          {memoryMib !== undefined && memoryMib !== null && memoryMib > 0 && (
            <Fact term={t.memory}>{formatBytes(memoryMib * MIB)}</Fact>
          )}
        </Facts>
      )}
    </Panel>
  );
}
