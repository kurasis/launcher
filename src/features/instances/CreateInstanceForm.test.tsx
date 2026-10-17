import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type {
  CatalogView,
  PlanSummary,
  VersionAvailability,
  VersionView,
} from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { CreateInstanceForm } from './CreateInstanceForm';

function catalog(availability: VersionAvailability, stale = false): CatalogView {
  const version = (
    id: string,
    versionType: VersionView['versionType'],
    verified = false,
  ): VersionView => ({
    id,
    versionType,
    releaseTime: '2024-08-08T12:00:00+00:00',
    verified,
    availability,
  });
  return {
    versions: [
      version('26.4-snapshot-4', 'snapshot'),
      version('1.21.1', 'release', true),
      version('1.12.2', 'release'),
      version('b1.7.3', 'old_beta'),
      version('rd-132211', 'old_alpha'),
    ],
    fetchedAt: '2026-10-17T00:00:00Z',
    stale,
    error: null,
    latestRelease: '1.21.1',
    latestSnapshot: '26.4-snapshot-4',
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
  render(<CreateInstanceForm onDone={() => undefined} />);
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

  it('separates version types into tabs and searches within one', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    expect(screen.getByLabelText('Name')).toHaveValue('1.21.1');
    const releases = screen.getByRole('list', { name: 'Releases' });
    expect(releases).toHaveTextContent('1.21.1');
    expect(releases).toHaveTextContent('Verified');
    expect(releases).not.toHaveTextContent('26.4-snapshot-4');
    expect(screen.getByRole('tab', { name: /Releases/ })).toHaveTextContent('2');

    fireEvent.click(screen.getByRole('tab', { name: /Snapshots/ }));
    expect(screen.getByText(/can damage worlds/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: /26\.4-snapshot-4/ }));
    expect(screen.getByText('Selected: 26.4-snapshot-4')).toBeVisible();
    expect(screen.getByLabelText('Name')).toHaveValue('26.4-snapshot-4');

    fireEvent.click(screen.getByRole('tab', { name: /Alpha/ }));
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'zzz' } });
    expect(screen.getByText('No version matches.')).toBeVisible();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'rd' } });
    fireEvent.click(screen.getByRole('button', { name: /rd-132211/ }));
    expect(screen.getByText(/no demo mode/)).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Create installation' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'create_instance')?.args).toMatchObject({
        request: { payload: { displayName: 'rd-132211', versionId: 'rd-132211' } },
      });
    });
  });

  it('does not offer an estimate without the catalog', async () => {
    setup(catalog('catalog_unavailable'));
    expect(await screen.findByText('Mojang catalog not reachable')).toBeVisible();
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
