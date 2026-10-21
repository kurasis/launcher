import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { AppSnapshot } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { installationOf } from './installations';

const t = strings.feed;

interface VersionActionProps {
  snapshot: AppSnapshot;
  version: string;
  variant?: 'primary' | 'secondary';
  /** Opens Installations with this installation selected. */
  onOpenInstallations: (instanceId: string | null) => void;
  /** Opens the new-installation form with this game version chosen. */
  onInstallVersion: (versionId: string) => void;
}

/**
 * Play when an installation of the version is ready, otherwise Install: it
 * opens the installation that is not ready yet, or the new-installation form.
 */
export function VersionAction({
  snapshot,
  version,
  variant = 'secondary',
  onOpenInstallations,
  onInstallVersion,
}: VersionActionProps) {
  const play = useCommand((requestId: string, id: string) => ipc.launchGame(id, requestId));
  const instance = installationOf(snapshot.instances, version);
  if (instance?.health === 'ready') {
    const running = snapshot.currentRun !== null;
    const busy = snapshot.activeOperation !== null;
    return (
      <>
        <Button
          variant={variant}
          disabled={running || busy || play.pending}
          onClick={() => void play.run(instance.id)}
        >
          {play.pending ? t.starting : t.playVersion({ version })}
        </Button>
        {running && <p role="status">{t.gameRunning}</p>}
        {play.error !== null && <Notice tone="danger">{play.error}</Notice>}
      </>
    );
  }
  return (
    <Button
      variant={variant}
      onClick={() => {
        if (instance !== null) onOpenInstallations(instance.id);
        else onInstallVersion(version);
      }}
    >
      {t.install({ version })}
    </Button>
  );
}
