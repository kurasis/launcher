import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import type { MicrosoftClientIdStatus } from '../../lib/contracts';
import { strings } from '../../lib/strings';

interface MicrosoftSetupPanelProps {
  status: MicrosoftClientIdStatus;
}

const tones: Record<MicrosoftClientIdStatus, StatusTone> = {
  not_configured: 'warning',
  invalid: 'danger',
  configured_unverified: 'neutral',
};

const t = strings.microsoftSetup;

/**
 * Reports whether this build carries a project-owned Microsoft client ID.
 * This is configuration status only; it never claims a working sign-in.
 */
export function MicrosoftSetupPanel({ status }: MicrosoftSetupPanelProps) {
  return (
    <Panel title={t.title} aside={<StatusBadge tone={tones[status]} label={t.status[status]} />}>
      <p>{t.explanation[status]}</p>
    </Panel>
  );
}
