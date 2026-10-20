import { act, fireEvent, render, screen } from '@testing-library/react';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { QuitControl } from './QuitControl';

function backend(results: unknown[]) {
  const actions: string[] = [];
  mockIPC(
    (cmd, args) => {
      if (cmd !== 'request_quit') return null;
      const { request } = args as { request: { payload: { action: string } } };
      actions.push(request.payload.action);
      return results.shift();
    },
    { shouldMockEvents: true },
  );
  return actions;
}

describe('QuitControl', () => {
  it('says why quitting failed even without the dialog', async () => {
    mockIPC((cmd) => {
      if (cmd === 'request_quit') {
        // The Rust core rejects with a plain error envelope, not an Error instance.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw { code: 'INTERNAL', message: 'Quitting failed.', diagnosticId: 'd-1' };
      }
      return null;
    });
    render(<QuitControl />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Quitting failed.');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('asks for a choice while a run is active and defaults to keeping it', async () => {
    const actions = backend([
      { status: 'choice_required', operationId: 'run-1' },
      { status: 'in_background' },
    ]);
    render(<QuitControl />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    const dialog = await screen.findByRole('dialog', { name: 'A run is still active' });
    const keep = screen.getByRole('button', { name: 'Keep running in background' });
    expect(keep).toHaveFocus();
    expect(dialog).toHaveTextContent('may lose unsaved progress');
    fireEvent.click(keep);
    await screen.findByRole('button', { name: 'Quit' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(actions).toEqual(['quit', 'keep_in_background']);
  });

  it('stops and quits only after explicit confirmation', async () => {
    const actions = backend([
      { status: 'choice_required', operationId: 'run-1' },
      { status: 'quitting' },
    ]);
    render(<QuitControl />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Stop and quit' }));
    await screen.findByRole('button', { name: 'Quit' });
    expect(actions).toEqual(['quit', 'stop_and_quit']);
  });

  it('cancel and Escape close the dialog without another request', async () => {
    const actions = backend([
      { status: 'choice_required', operationId: 'r' },
      { status: 'choice_required', operationId: 'r' },
    ]);
    render(<QuitControl />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    fireEvent.keyDown(await screen.findByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(actions).toEqual(['quit', 'quit']);
  });

  it('opens the choice when the OS or menu asks to quit during a run', async () => {
    const actions = backend([]);
    render(<QuitControl />);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
      await emit('quit_requested');
    });
    expect(await screen.findByRole('dialog', { name: 'A run is still active' })).toBeVisible();
    expect(actions).toEqual([]);
  });

  it('words the choice for an installation that is still running', async () => {
    const actions = backend([{ status: 'choice_required', operationId: 'op-1' }]);
    render(<QuitControl activeKind="install" />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    expect(
      await screen.findByRole('dialog', { name: 'Game files are still being prepared' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Cancel and quit' })).toBeVisible();
    expect(actions).toEqual(['quit']);
  });

  it('says the world closes for its players when only a world is open', async () => {
    const actions = backend([
      { status: 'choice_required', operationId: 'host-1', hosting: true },
      { status: 'quitting' },
    ]);
    render(<QuitControl />);
    fireEvent.click(screen.getByRole('button', { name: 'Quit' }));
    const dialog = await screen.findByRole('dialog', { name: 'Your world is open to friends' });
    expect(dialog).toHaveTextContent('closes it for everyone playing in it');
    expect(screen.getByRole('button', { name: 'Keep running in background' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Close the world and quit' }));
    await screen.findByRole('button', { name: 'Quit' });
    expect(actions).toEqual(['quit', 'stop_and_quit']);
  });

  it('adds the open world to the choice the OS or menu asks for during a run', async () => {
    backend([]);
    render(<QuitControl activeKind="launch" />);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
      await emit('quit_requested', { hosting: true });
    });
    const dialog = await screen.findByRole('dialog', { name: 'A run is still active' });
    expect(dialog).toHaveTextContent('closes it for everyone playing in it');
  });
});
