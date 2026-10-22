import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type {
  CatalogView,
  LoaderKind,
  LoaderVersion,
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

const loaderGames: Record<string, string[]> = {
  fabric: ['1.21.1', '1.21'],
  forge: ['1.12.2', '1.20.1'],
  // Supports nothing the catalog lists.
  quilt: ['1.0'],
};

const loaderVersions: Record<string, LoaderVersion[]> = {
  'fabric:1.21.1': [
    { id: '0.17.0-beta.1', label: '0.17.0-beta.1', stable: false, recommended: false },
    { id: '0.16.14', label: '0.16.14', stable: true, recommended: true },
  ],
  'forge:1.12.2': [
    { id: '1.12.2-14.23.5.2860', label: '14.23.5.2860', stable: true, recommended: false },
    { id: '1.12.2-14.23.5.2859', label: '14.23.5.2859', stable: true, recommended: true },
  ],
};

interface SetupOptions {
  onDone?: (createdId?: string) => void;
  initialLoader?: LoaderKind | null;
  /** Answers instead of the fixtures; return undefined to use them. */
  override?: (cmd: string, args: unknown) => unknown;
}

function setup(view: CatalogView, options: SetupOptions = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const overridden = options.override?.(cmd, args);
    if (overridden !== undefined) return overridden;
    const a = args as { kind?: string; gameVersion?: string };
    if (cmd === 'list_loader_game_versions') return loaderGames[a.kind ?? ''] ?? [];
    if (cmd === 'list_loader_versions')
      return loaderVersions[`${a.kind ?? ''}:${a.gameVersion ?? ''}`] ?? [];
    if (cmd === 'list_supported_versions') return view;
    if (cmd === 'preview_install_plan') return plan;
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  render(
    <CreateInstanceForm
      onDone={options.onDone ?? (() => undefined)}
      initialLoader={options.initialLoader ?? null}
    />,
  );
  return calls;
}

/** A promise the test settles when it wants. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe('CreateInstanceForm', () => {
  it('fetches the version list again when the connection returns', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    const second = deferred<CatalogView>();
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'list_supported_versions') {
        return (args as { refresh: boolean }).refresh ? second.promise : catalog('available');
      }
      if (cmd === 'recommend_memory') return null;
      return [];
    });
    const { rerender } = render(<CreateInstanceForm network="offline" onDone={() => undefined} />);
    const lists = () => calls.filter((c) => c.cmd === 'list_supported_versions');
    await waitFor(() => {
      expect(lists()).toHaveLength(1);
    });
    expect(lists()[0]?.args).toMatchObject({ refresh: false });
    expect(await screen.findByText('Available from Mojang')).toBeVisible();
    rerender(<CreateInstanceForm network="online" onDone={() => undefined} />);
    await waitFor(() => {
      expect(lists()).toHaveLength(2);
    });
    expect(lists()[1]?.args).toMatchObject({ refresh: true });
    // It says that it checks while the answer is on its way.
    expect(screen.getByText('Checking the Mojang catalog…')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Check again' })).toBeDisabled();
    second.resolve(catalog('available'));
    expect(await screen.findByText('Available from Mojang')).toBeVisible();
  });

  it('drops a catalog answer that a newer request overtook', async () => {
    const first = deferred<CatalogView>();
    let asked = 0;
    mockIPC((cmd) => {
      if (cmd === 'list_supported_versions') {
        asked += 1;
        return asked === 1 ? first.promise : catalog('available');
      }
      return null;
    });
    const { rerender } = render(<CreateInstanceForm network="offline" onDone={() => undefined} />);
    await waitFor(() => {
      expect(asked).toBe(1);
    });
    // Back online before the first answer: the second request answers first.
    rerender(<CreateInstanceForm network="online" onDone={() => undefined} />);
    expect(await screen.findByText('Available from Mojang')).toBeVisible();
    first.resolve(catalog('catalog_unavailable'));
    await first.promise;
    await waitFor(() => {
      expect(screen.getByText('Available from Mojang')).toBeVisible();
    });
    expect(screen.queryByText('Mojang catalog not reachable')).toBeNull();
  });

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
      loader: null,
    });
  });

  it('drops an estimate that arrives after the version changed', async () => {
    const answer = deferred<PlanSummary>();
    setup(catalog('available'), {
      override: (cmd) => (cmd === 'preview_install_plan' ? answer.promise : undefined),
    });
    expect(await screen.findByText('Available from Mojang')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Estimate download' }));
    expect(screen.getByRole('button', { name: 'Estimating…' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: /^1\.12\.2/ }));
    answer.resolve(plan);
    expect(await screen.findByRole('button', { name: 'Estimate download' })).toBeEnabled();
    expect(screen.queryByText('Still to download')).toBeNull();
  });

  it('says the chosen version and its availability on one status line', async () => {
    setup(catalog('available'));
    const chosen = await screen.findByText('Selected: 1.21.1');
    const line = chosen.closest('[role="status"]');
    expect(line).not.toBeNull();
    expect(line).toHaveTextContent('Selected: 1.21.1 · Available from Mojang');
    expect(within(line as HTMLElement).queryAllByRole('status')).toHaveLength(0);
  });

  it('separates version types into tabs and searches within one', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    expect(screen.getByLabelText('Name')).toHaveValue('1.21.1');
    expect(screen.getByLabelText('Name')).toHaveAccessibleDescription(
      'Filled in from the version. You can rename it later.',
    );
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

  it('offers the mod loaders as one group of pills whose hint follows the pick', async () => {
    setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    const group = screen.getByRole('group', { name: 'Mod loader' });
    expect(
      within(group)
        .getAllByRole('radio')
        .map((radio) => radio.getAttribute('value')),
    ).toEqual(['none', 'fabric', 'quilt', 'forge', 'neoforge', 'optifine']);
    expect(within(group).getByRole('radio', { name: 'None' })).toBeChecked();
    expect(group).toHaveAccessibleDescription('The unmodified game.');
    expect(screen.queryByLabelText('Fabric version')).toBeNull();

    fireEvent.click(within(group).getByRole('radio', { name: 'Fabric' }));
    expect(within(group).getByRole('radio', { name: 'Fabric' })).toBeChecked();
    expect(within(group).getByRole('radio', { name: 'None' })).not.toBeChecked();
    expect(group).toHaveAccessibleDescription('A lightweight loader for Fabric mods.');
    expect(await screen.findByLabelText('Fabric version')).toBeVisible();

    fireEvent.click(within(group).getByRole('radio', { name: 'None' }));
    expect(group).toHaveAccessibleDescription('The unmodified game.');
    expect(screen.queryByLabelText('Fabric version')).toBeNull();
    expect(screen.getByLabelText('Name')).toHaveValue('1.21.1');
  });

  it('installs a mod loader with a chosen game and loader version', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Fabric' }));
    expect(screen.getByRole('radio', { name: 'Fabric' })).toBeChecked();
    expect(
      await screen.findByText('Only Minecraft versions that Fabric supports are listed.'),
    ).toBeVisible();
    // Only what Fabric supports; 1.12.2 is hidden.
    const releases = screen.getByRole('list', { name: 'Releases' });
    expect(releases).toHaveTextContent('1.21.1');
    expect(releases).not.toHaveTextContent('1.12.2');
    const version = screen.getByLabelText('Fabric version');
    await waitFor(() => {
      expect(version).toHaveValue('0.16.14');
    });
    expect(screen.getByRole('option', { name: '0.16.14 (recommended)' })).toBeVisible();
    expect(screen.getByRole('option', { name: '0.17.0-beta.1 (beta)' })).toBeVisible();
    expect(screen.getByLabelText('Name')).toHaveValue('1.21.1 Fabric');
    fireEvent.change(version, { target: { value: '0.17.0-beta.1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create installation' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'create_instance')?.args).toMatchObject({
        request: {
          payload: {
            displayName: '1.21.1 Fabric',
            versionId: '1.21.1',
            loader: { kind: 'fabric', version: '0.17.0-beta.1' },
          },
        },
      });
    });
  });

  it('moves to a game version the loader supports', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Forge' }));
    expect(screen.getByRole('radio', { name: 'Forge' })).toBeChecked();
    expect(await screen.findByText('Selected: 1.12.2')).toBeVisible();
    await waitFor(() => {
      expect(screen.getByLabelText('Forge version')).toHaveValue('1.12.2-14.23.5.2859');
    });
    expect(screen.getByRole('option', { name: '14.23.5.2859 (recommended)' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Estimate download' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'preview_install_plan')?.args).toEqual({
        versionId: '1.12.2',
        loader: { kind: 'forge', version: '1.12.2-14.23.5.2859' },
      });
    });
  });

  it('starts with the loader asked for and lists only what it supports', async () => {
    setup(catalog('available'), { initialLoader: 'fabric' });
    const fabric = await screen.findByRole('radio', { name: 'Fabric' });
    expect(fabric).toBeChecked();
    expect(
      await screen.findByText('Only Minecraft versions that Fabric supports are listed.'),
    ).toBeVisible();
    const releases = screen.getByRole('list', { name: 'Releases' });
    expect(releases).toHaveTextContent('1.21.1');
    expect(releases).not.toHaveTextContent('1.12.2');
    // Fabric supports no snapshot, beta or alpha here: no tabs to choose from.
    expect(screen.queryByRole('tablist')).toBeNull();
    await waitFor(() => {
      expect(screen.getByLabelText('Fabric version')).toHaveValue('0.16.14');
    });
    expect(screen.getByLabelText('Name')).toHaveValue('1.21.1 Fabric');
  });

  it('moves to a supported version when the loader answers before the catalog', async () => {
    const view = deferred<CatalogView>();
    const calls = setup(catalog('available'), {
      initialLoader: 'forge',
      override: (cmd) => (cmd === 'list_supported_versions' ? view.promise : undefined),
    });
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'list_loader_game_versions')).toBe(true);
    });
    expect(screen.getByText('Choose a version.')).toBeVisible();
    view.resolve(catalog('available'));
    expect(await screen.findByText('Selected: 1.12.2')).toBeVisible();
    await waitFor(() => {
      expect(screen.getByLabelText('Forge version')).toHaveValue('1.12.2-14.23.5.2859');
    });
  });

  it('goes back to the installations from the back button and Cancel', async () => {
    const onDone = vi.fn();
    setup(catalog('available'), { onDone });
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    expect(screen.getByRole('heading', { level: 2, name: 'New installation' })).toHaveFocus();
    const back = screen.getByRole('button', { name: 'Back to installations' });
    expect(back).toHaveAttribute('title', 'Back to installations');
    fireEvent.click(back);
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(onDone).toHaveBeenLastCalledWith();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onDone).toHaveBeenCalledTimes(2);
  });

  it('tries a failed loader request again and clears its error', async () => {
    let fails = 1;
    const calls = setup(catalog('available'), {
      override: (cmd) => {
        if (cmd === 'list_loader_versions' && fails > 0) {
          fails -= 1;
          return Promise.reject(new Error('The Fabric servers did not answer.'));
        }
        return undefined;
      },
    });
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Fabric' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The Fabric servers did not answer.',
    );
    expect(screen.getByLabelText('Fabric version')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Create installation' })).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Fabric version')).toHaveValue('0.16.14');
    });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Try again' })).toBeNull();
    expect(calls.filter((c) => c.cmd === 'list_loader_versions')).toHaveLength(2);
    expect(screen.getByRole('button', { name: 'Create installation' })).toBeEnabled();
  });

  it('clears a failed loader list once the same loader answers', async () => {
    let fails = 1;
    setup(catalog('available'), {
      override: (cmd) => {
        if (cmd === 'list_loader_game_versions' && fails > 0) {
          fails -= 1;
          return Promise.reject(new Error('Fabric is not reachable.'));
        }
        return undefined;
      },
    });
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Fabric' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Fabric is not reachable.');
    // Another loader and back: the request runs again and its success clears the error.
    fireEvent.click(screen.getByRole('radio', { name: 'Forge' }));
    expect(screen.queryByRole('alert')).toBeNull();
    fireEvent.click(screen.getByRole('radio', { name: 'Fabric' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Fabric version')).toHaveValue('0.16.14');
    });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('says so when the loader supports no version in the catalog', async () => {
    setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: 'Quilt' }));
    expect(
      await screen.findByText(
        'Quilt supports none of the versions in the Mojang catalog. Choose another loader or check the catalog again.',
      ),
    ).toBeVisible();
    expect(screen.queryByRole('list', { name: 'Releases' })).toBeNull();
    expect(screen.getByText('Quilt has no version for Minecraft 1.21.1.')).toBeVisible();
    expect(screen.getByLabelText('Quilt version')).toBeDisabled();
    expect(screen.queryByText('Loading versions…')).toBeNull();
    expect(screen.getByRole('button', { name: 'Create installation' })).toBeDisabled();
  });

  it('keeps a typed name within the limit the core accepts', async () => {
    const calls = setup(catalog('available'));
    expect(await screen.findByText('Selected: 1.21.1')).toBeVisible();
    const name = screen.getByLabelText('Name');
    expect(name).toHaveAttribute('maxLength', '64');
    const junk = `<img src=x onerror=alert(1)>${'é'.repeat(200)}`;
    fireEvent.change(name, { target: { value: junk } });
    expect((name as HTMLInputElement).value).toBe(junk.slice(0, 64));
    expect(document.querySelector('img')).toBeNull();
    fireEvent.change(name, { target: { value: '   ' } });
    expect(screen.getByRole('button', { name: 'Create installation' })).toBeDisabled();
    fireEvent.change(name, { target: { value: '  Creative  ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create installation' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'create_instance')?.args).toMatchObject({
        request: { payload: { displayName: '  Creative  ', versionId: '1.21.1' } },
      });
    });
  });
});
