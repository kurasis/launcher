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
});
