import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { instanceFixture } from '../../test/fixtures';
import { InstanceDetails } from './InstanceDetails';

describe('InstanceDetails', () => {
  it('saves notes for the installation', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'set_instance_notes') {
        const payload = (args as { request: { payload: { notes: string } } }).request.payload;
        return instanceFixture({ notes: payload.notes.trim() });
      }
      return null;
    });
    const { rerender } = render(
      <InstanceDetails instance={instanceFixture()} onEditSettings={() => undefined} />,
    );
    const save = screen.getByRole('button', { name: 'Save notes' });
    expect(save).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Notes'), {
      target: { value: 'Create mod pack\nserver: example.org ' },
    });
    fireEvent.click(save);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_instance_notes')?.args).toMatchObject({
        request: {
          payload: {
            instanceId: instanceFixture().id,
            notes: 'Create mod pack\nserver: example.org ',
          },
        },
      });
    });
    rerender(
      <InstanceDetails
        instance={instanceFixture({ notes: 'Create mod pack\nserver: example.org' })}
        onEditSettings={() => undefined}
      />,
    );
    expect(await screen.findByText('Saved.')).toBeVisible();
    expect(save).toBeDisabled();
  });

  it('renames the installation and opens its folders', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'rename_instance') {
        return instanceFixture({ displayName: 'With friends' });
      }
      return null;
    });
    const { rerender } = render(
      <InstanceDetails instance={instanceFixture()} onEditSettings={() => undefined} />,
    );
    // A vanilla installation has no mods folder to offer.
    expect(screen.queryByRole('button', { name: 'Mods' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Worlds' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'open_instance_folder')?.args).toMatchObject({
        request: { payload: { instanceId: instanceFixture().id, folder: 'saves' } },
      });
    });
    const rename = screen.getByRole('button', { name: 'Rename' });
    expect(rename).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: '   ' } });
    expect(rename).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: ' With friends ' } });
    fireEvent.click(rename);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'rename_instance')?.args).toMatchObject({
        request: { payload: { instanceId: instanceFixture().id, displayName: ' With friends ' } },
      });
    });
    rerender(
      <InstanceDetails
        instance={instanceFixture({
          displayName: 'With friends',
          loader: { kind: 'fabric', version: '0.16.0' },
        })}
        onEditSettings={() => undefined}
      />,
    );
    expect(await screen.findByRole('button', { name: 'Mods' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Shader packs' })).toBeVisible();
  });
});
