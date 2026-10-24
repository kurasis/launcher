import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import { instanceFixture } from '../../test/fixtures';
import { InstanceDetails } from './InstanceDetails';

function backend(answer: (cmd: string) => unknown = () => null) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return answer(cmd);
  });
  return calls;
}

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
    // The notes are read first; "Edit" opens the editor.
    expect(screen.getByText('No notes yet.')).toBeVisible();
    expect(screen.queryByLabelText('Notes')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Edit notes' }));
    const save = screen.getByRole('button', { name: 'Save notes' });
    expect(save).toBeDisabled();
    expect(screen.getByLabelText('Notes')).toHaveFocus();
    expect(screen.getByLabelText('Notes')).toHaveAttribute('maxLength', '4000');
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
    // The editor closed: the notes show as text and "Edit" has focus again.
    expect(screen.queryByLabelText('Notes')).toBeNull();
    expect(screen.getByText(/Create mod pack/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Edit notes' })).toHaveFocus();
  });

  it('cancels editing the notes without saving', () => {
    const calls = backend();
    render(
      <InstanceDetails
        instance={instanceFixture({ notes: 'Old notes' })}
        onEditSettings={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Edit notes' }));
    fireEvent.change(screen.getByLabelText('Notes'), { target: { value: '<b>new</b>' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByText('Old notes')).toBeVisible();
    // Escape closes the editor too.
    fireEvent.click(screen.getByRole('button', { name: 'Edit notes' }));
    fireEvent.keyDown(screen.getByLabelText('Notes'), { key: 'Escape' });
    expect(screen.queryByLabelText('Notes')).toBeNull();
    expect(calls.some((c) => c.cmd === 'set_instance_notes')).toBe(false);
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
    // Rename is in the installation's menu and opens under its head.
    expect(screen.queryByLabelText('Name')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Installation actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    const rename = screen.getByRole('button', { name: 'Rename' });
    expect(screen.getByLabelText('Name')).toHaveFocus();
    expect(screen.getByLabelText('Name')).toHaveAttribute('maxLength', '64');
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
    // Renamed: the form closed and the new name, a heading, has focus.
    expect(screen.queryByLabelText('Name')).toBeNull();
    expect(screen.getByRole('heading', { name: 'With friends' })).toHaveFocus();
  });

  it('shows memory and window as separate facts, with the launcher skins', () => {
    backend();
    const { rerender } = render(
      <InstanceDetails
        instance={instanceFixture({ jvmArguments: '-XX:+UseZGC' })}
        onEditSettings={() => undefined}
      />,
    );
    const facts = screen.getByText('Memory').closest('dl');
    if (facts === null) throw new Error('No facts list.');
    expect(within(facts).getByText('4096 MiB')).toBeVisible();
    expect(within(facts).getByText('1280×720')).toBeVisible();
    expect(within(facts).getByText('On')).toBeVisible();
    expect(within(facts).getByText('-XX:+UseZGC')).toHaveAttribute('dir', 'ltr');
    rerender(
      <InstanceDetails
        instance={instanceFixture({ fullscreen: true, launcherSkins: false })}
        onEditSettings={() => undefined}
      />,
    );
    expect(within(facts).getByText('1280×720, fullscreen')).toBeVisible();
    expect(within(facts).getByText('Off')).toBeVisible();
  });

  it('keeps the actions in a toolbar and runs them for this installation', async () => {
    const calls = backend((cmd) => (cmd === 'repair_instance' ? { operationId: 'op-1' } : null));
    const onEditSettings = vi.fn();
    const onOpenContent = vi.fn();
    const onOpenBackups = vi.fn();
    const ready = instanceFixture({ health: 'ready' });
    const { rerender } = render(
      <InstanceDetails
        instance={ready}
        onEditSettings={onEditSettings}
        onOpenContent={onOpenContent}
        onOpenBackups={onOpenBackups}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Installation settings' }));
    fireEvent.click(screen.getByRole('button', { name: 'Mods and content' }));
    fireEvent.click(screen.getByRole('button', { name: 'Backups' }));
    expect(onEditSettings).toHaveBeenCalledOnce();
    expect(onOpenContent).toHaveBeenCalledOnce();
    expect(onOpenBackups).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Game folder' }));
    fireEvent.click(screen.getByRole('button', { name: 'Verify files' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'repair_instance')).toBeDefined();
    });
    expect(calls.find((c) => c.cmd === 'open_instance_folder')?.args).toMatchObject({
      request: { payload: { instanceId: ready.id, folder: null } },
    });
    // While its own operation runs there is nothing to verify; queued, it waits.
    rerender(
      <InstanceDetails
        instance={ready}
        onEditSettings={onEditSettings}
        activeOperation={{ kind: 'install', instanceId: ready.id, operationId: 'op-2' }}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Verify files' })).toBeNull();
    rerender(
      <InstanceDetails
        instance={ready}
        onEditSettings={onEditSettings}
        activeOperation={{ kind: 'install', instanceId: 'other', operationId: 'op-3' }}
        queue={[
          {
            queueId: 'q-1',
            instanceId: ready.id,
            kind: 'repair',
            queuedAt: '2026-10-20T19:00:00Z',
          },
        ]}
      />,
    );
    const queuedVerify = screen.getByRole('button', { name: 'Verify files' });
    expect(queuedVerify).toHaveAttribute('aria-disabled', 'true');
    fireEvent.click(queuedVerify);
    expect(calls.filter((c) => c.cmd === 'repair_instance')).toHaveLength(1);
    // A vanilla installation that is not installed has no files to verify yet.
    rerender(<InstanceDetails instance={instanceFixture()} onEditSettings={onEditSettings} />);
    expect(screen.queryByRole('button', { name: 'Verify files' })).toBeNull();
  });
});
