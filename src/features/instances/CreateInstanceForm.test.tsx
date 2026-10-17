import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { CatalogView, PlanSummary, VersionAvailability } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { CreateInstanceForm } from './CreateInstanceForm';

function catalog(availability: VersionAvailability, stale = false): CatalogView {
  return {
    versions: [
      {
        id: '1.21.1',
        status: 'pending_verification',
        expectedJavaMajor: 21,
        availability,
        catalogSha1: null,
      },
    ],
    fetchedAt: '2026-10-17T00:00:00Z',
    stale,
    error: null,
    latestRelease: '26.3',
  };
}

const plan: PlanSummary = {
  versionId: '1.21.1',
  descriptorSha1: '499680d659b7a0ccb2fa9a809c345c5baf2ace7d',
  javaMajor: 21,
  libraries: 56,
  assetObjects: 3888,
  gameArtifacts: 3947,
  gameBytes: 900 * 1024 * 1024,
  missingGameBytes: 900 * 1024 * 1024,
  runtimeBytes: null,
};

function setup(view: CatalogView) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'list_supported_versions') return view;
    if (cmd === 'preview_install_plan') return plan;
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  const versions = snapshotFixture().supportedVersions.filter((v) => v.id === '1.21.1');
  render(<CreateInstanceForm versions={versions} onDone={() => undefined} />);
  return calls;
}

describe('CreateInstanceForm', () => {
  it('shows availability and a metadata-only download estimate', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Available from Mojang')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Estimate download' }));
    const summary = await screen.findByRole('region', { name: 'Download estimate for 1.21.1' });
    expect(summary).toHaveTextContent('Java21');
    expect(summary).toHaveTextContent('Asset files3888');
    expect(summary).toHaveTextContent('Still to download900.0 MiB');
    expect(summary).toHaveTextContent('Determined when the runtime is prepared');
    expect(summary).toHaveTextContent(/not yet been checked by starting the game/);
    expect(calls.find((c) => c.cmd === 'preview_install_plan')?.args).toEqual({
      versionId: '1.21.1',
    });
  });

  it('does not offer an estimate for a changed descriptor', async () => {
    setup(catalog('descriptor_changed'));
    expect(await screen.findByText(/Changed upstream/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Estimate download' })).toBeDisabled();
  });

  it('explains a stale catalog and can force a refresh', async () => {
    const calls = setup(catalog('available', true));
    expect(await screen.findByText(/last saved catalog/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'list_supported_versions').at(-1)?.args).toEqual({
        refresh: true,
      });
    });
  });
});
