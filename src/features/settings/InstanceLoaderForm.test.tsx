import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { LoaderChoice, LoaderVersion } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstanceLoaderForm } from './InstanceLoaderForm';

const forge: LoaderChoice = { kind: 'forge', version: '1.21.1-52.0.1' };
const instance = instanceFixture({ versionId: '1.21.1', health: 'ready', loader: forge });
const versions: LoaderVersion[] = [
  { id: '1.21.1-52.1.0', label: '52.1.0', stable: true, recommended: true },
  { id: '1.21.1-52.0.9', label: '52.0.9', stable: false, recommended: false },
];

function backend() {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'list_loader_versions') return versions;
    if (cmd === 'set_instance_loader') {
      return {
        ...instance,
        health: 'not_installed',
        loader: { kind: 'forge', version: '1.21.1-52.1.0' },
      };
    }
    if (cmd === 'install_instance') return { operationId: 'op-1' };
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return calls;
}

describe('InstanceLoaderForm', () => {
  it('switches to another loader version and installs it', async () => {
    const calls = backend();
    render(<InstanceLoaderForm instance={instance} loader={forge} queued={false} />);
    expect(screen.getByText('Now: Forge 52.0.1')).toBeVisible();
    const picker = screen.getByLabelText('Forge version');
    // The installed version stays listed although the project no longer offers it.
    await waitFor(() => {
      expect(
        Array.from(
          screen.getByLabelText<HTMLSelectElement>('Forge version').options,
          (o) => o.textContent,
        ),
      ).toEqual(['52.0.1', '52.1.0 (recommended)', '52.0.9 (beta)']);
    });
    const apply = screen.getByRole('button', { name: 'Change and install' });
    expect(apply).toBeDisabled();

    fireEvent.change(picker, { target: { value: '1.21.1-52.1.0' } });
    expect(
      screen.getByText(/Mods made for Forge 52\.0\.1 may not work with 52\.1\.0/),
    ).toBeVisible();
    fireEvent.click(apply);
    expect(await screen.findByText('Switched to Forge 52.1.0; installing.')).toBeVisible();
    const order = calls
      .map((c) => c.cmd)
      .filter((c) => c.startsWith('set_') || c.startsWith('install'));
    expect(order).toEqual(['set_instance_loader', 'install_instance']);
    expect(calls.find((c) => c.cmd === 'set_instance_loader')?.args).toMatchObject({
      request: { payload: { instanceId: instance.id, version: '1.21.1-52.1.0' } },
    });
  });

  it('leaves a queued install to pick the new version up', async () => {
    const calls = backend();
    render(<InstanceLoaderForm instance={instance} loader={forge} queued />);
    const picker = await screen.findByRole('option', { name: '52.0.9 (beta)' });
    fireEvent.change(picker.parentElement as HTMLSelectElement, {
      target: { value: '1.21.1-52.0.9' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Change and install' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'set_instance_loader')).toBe(true);
    });
    expect(calls.some((c) => c.cmd === 'install_instance')).toBe(false);
  });

  it('is unavailable while the installation is busy', () => {
    backend();
    render(
      <InstanceLoaderForm instance={{ ...instance, busy: true }} loader={forge} queued={false} />,
    );
    expect(screen.getByLabelText('Forge version')).toBeDisabled();
  });
});
