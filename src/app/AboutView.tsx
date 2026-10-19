import { Button } from '../components/Button';
import { Notice } from '../components/Notice';
import { Panel } from '../components/Panel';
import { SystemInfoPanel } from '../features/diagnostics/SystemInfoPanel';
import type { ExternalLink } from '../lib/contracts';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { useShownError } from '../lib/useShownError';
import styles from './App.module.css';
import { useAppInfo } from './useAppInfo';

const t = strings.about;

function LinkButton({ link, label }: { link: ExternalLink; label: string }) {
  const opening = useShownError();
  return (
    <>
      <Button
        onClick={() => {
          opening.clear();
          void ipc.openExternal(link).catch(opening.catcher);
        }}
      >
        {label}
      </Button>
      {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
    </>
  );
}

export function AboutView() {
  const { state, reload } = useAppInfo();
  return (
    <>
      <Panel title={t.title}>
        <p>{strings.foundation.body}</p>
        <p>{strings.independentNotice}</p>
        <p>{t.license}</p>
        <div className={styles.linkRow}>
          <LinkButton link="source_code" label={t.sourceCode} />
        </div>
      </Panel>
      <Panel title={t.officialTitle}>
        <div className={styles.linkRow}>
          <LinkButton link="minecraft_eula" label={t.eula} />
          <LinkButton link="minecraft_help" label={t.help} />
        </div>
      </Panel>
      <SystemInfoPanel state={state} onRetry={reload} />
    </>
  );
}
