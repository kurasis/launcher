import type { StatusTone } from '../../components/StatusBadge';
import type { InstallationHealth } from '../../lib/contracts';

export const healthTone: Record<InstallationHealth, StatusTone> = {
  not_installed: 'neutral',
  ready: 'success',
  needs_verification: 'warning',
  repair_required: 'danger',
  unsupported: 'danger',
};
