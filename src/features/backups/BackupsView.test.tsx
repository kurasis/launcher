import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, BackupInfo, BackupOverview, BackupScope } from '../../lib/contracts';
import { loadLanguage, setLanguage, strings } from '../../lib/strings';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { BackupsView } from './BackupsView';

const instance = instanceFixture({ health: 'ready' });

function backup(overrides: Partial<BackupInfo> = {}): BackupInfo {
  return {
    id: '7d5c2a1e-0000-4000-8000-000000000001',
    instanceId: instance.id,
    instanceName: 'Survival',
    versionLabel: 'Minecraft 1.21.1',
    scope: { kind: 'world', name: 'My World' },
    reason: 'manual',
    createdAt: '2026-10-17T12:00:00Z',
    fileCount: 42,
    totalBytes: 5 * 1024 * 1024,
    newBytes: 1024 * 1024,
    ...overrides,
  };
}

type Answers = Record<string, (args: unknown) => unknown>;

const shown = (overrides: Partial<AppSnapshot> = {}) =>
  snapshotFixture({ instances: [instance], ...overrides });

function setup(overview: BackupOverview, answers: Answers = {}, snapshot = shown()) {
  const calls: { cmd: string; args: unknown }[] = [];
  // As the store does: a deleted backup is gone from the next list.
  let found = overview;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(args);
    if (cmd === 'list_backups') return found;
    if (cmd === 'create_backup') return backup({ id: 'new' });
    if (cmd === 'restore_backup') return { world: 'My World (backup 2026-10-17 12-00)' };
    if (cmd === 'delete_backup') {
      const { backupId } = (args as { request: { payload: { backupId: string } } }).request.payload;
      found = { ...found, backups: found.backups.filter((b) => b.id !== backupId) };
      return 1024 * 1024;
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  const view = render(<BackupsView snapshot={snapshot} />);
  return { calls, rerender: view.rerender };
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

const oneWorld: BackupOverview = {
  backups: [backup()],
  worlds: [{ instanceId: instance.id, worlds: ['My World'] }],
  storeBytes: 5 * 1024 * 1024,
  deletedBytes: [],
  deletedTotalBytes: 0,
};

/** The answer inside the question, not the row's button of the same name. */
const answer = (name: string) =>
  within(screen.getByRole('alertdialog')).getByRole('button', { name });

afterEach(() => {
  vi.useRealTimers();
  setLanguage('en');
});

describe('BackupsView', () => {
  it('backs up a world or the whole installation', async () => {
    const { calls } = setup({
      backups: [],
      worlds: [{ instanceId: instance.id, worlds: ['My World'] }],
      storeBytes: 0,
      deletedBytes: [],
      deletedTotalBytes: 0,
    });
    expect(await screen.findByText('My World')).toBeVisible();
    expect(screen.getByText('Not backed up yet')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back up' }));
    await waitFor(() => {
      expect(named('create_backup', calls)[0]?.args).toMatchObject({
        request: {
          payload: { instanceId: instance.id, scope: { kind: 'world', name: 'My World' } },
        },
      });
    });
    expect(await screen.findByText('World “\u2068My World\u2069” backed up.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Back up the whole installation' }));
    await waitFor(() => {
      expect(named('create_backup', calls)[1]?.args).toMatchObject({
        request: { payload: { scope: { kind: 'installation' } } },
      });
    });
  });

  it('lists backups and restores or deletes them after confirmation', async () => {
    const { calls } = setup(oneWorld);
    expect(await screen.findByText('Backups use \u20685.0 MiB\u2069')).toBeVisible();
    expect(
      screen.getByText('42 files · \u20685.0 MiB\u2069 · \u20681.0 MiB\u2069 new'),
    ).toBeVisible();
    expect(screen.getByText('Made by you')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Restore' }));
    expect(screen.getByText(/Its current state is backed up first/)).toBeVisible();
    expect(named('restore_backup', calls)).toHaveLength(0);
    fireEvent.click(answer('Restore'));
    await waitFor(() => {
      expect(named('restore_backup', calls)[0]?.args).toMatchObject({
        request: { payload: { backupId: backup().id, asCopy: false } },
      });
    });
    expect(await screen.findByText('Restored.')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Restore as a copy' }));
    expect(
      await screen.findByText(
        'Restored as the world “\u2068My World (backup 2026-10-17 12-00)\u2069”.',
      ),
    ).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(answer('Delete'));
    expect(await screen.findByText('Deleted; \u20681.0 MiB\u2069 freed.')).toBeVisible();
    expect(await screen.findByText('No backups yet.')).toBeVisible();
    // The row that had the focus went: the title takes it.
    expect(screen.getByRole('heading', { name: 'Backups', level: 2 })).toHaveFocus();
  });

  it('gives the focus to the list when a deleted row goes, and back to the button that asked', async () => {
    setup({
      ...oneWorld,
      backups: [backup(), backup({ id: 'older', createdAt: '2026-10-16T12:00:00Z' })],
    });
    const [restore] = await screen.findAllByRole('button', { name: 'Restore' });
    fireEvent.click(restore as HTMLElement);
    fireEvent.keyDown(screen.getByRole('alertdialog'), { key: 'Escape' });
    expect(screen.queryByRole('alertdialog')).toBeNull();
    await waitFor(() => {
      expect(restore).toHaveFocus();
    });

    const [remove] = screen.getAllByRole('button', { name: 'Delete' });
    fireEvent.click(remove as HTMLElement);
    fireEvent.click(answer('Cancel'));
    await waitFor(() => {
      expect(remove).toHaveFocus();
    });

    fireEvent.click(remove as HTMLElement);
    fireEvent.click(answer('Delete'));
    const list = screen.getByRole('list', { name: /Backups of this installation/ });
    await waitFor(() => {
      expect(within(list).getAllByRole('listitem')).toHaveLength(1);
    });
    await waitFor(() => {
      expect(list).toHaveFocus();
    });
  });

  it('keeps a pressed button focusable while its own backup goes', async () => {
    let finish: () => void = () => undefined;
    const { calls, rerender } = setup(oneWorld, {
      create_backup: (args) =>
        new Promise((resolve) => {
          const { scope } = (args as { request: { payload: { scope: BackupScope } } }).request
            .payload;
          finish = () => {
            resolve(backup({ id: 'new', scope }));
          };
        }),
    });
    const backUp = await screen.findByRole('button', { name: 'Back up' });
    backUp.focus();
    fireEvent.click(backUp);
    expect(backUp).toHaveAttribute('aria-disabled', 'true');
    expect(backUp).toBeEnabled();
    expect(backUp).toHaveFocus();
    expect(screen.getByText('Working…')).toBeVisible();
    // The backup shows in the snapshot as the launcher's activity: still this screen's.
    const ours = shown({
      activeOperation: { kind: 'backup', instanceId: instance.id, operationId: 'op-1' },
    });
    rerender(<BackupsView snapshot={ours} />);
    expect(backUp).toBeEnabled();
    expect(screen.queryByText(/wait until the running operation/)).toBeNull();
    // A second press does nothing.
    fireEvent.click(screen.getByRole('button', { name: 'Back up the whole installation' }));
    expect(named('create_backup', calls)).toHaveLength(1);
    finish();
    expect(await screen.findByText('World “\u2068My World\u2069” backed up.')).toBeVisible();
    // The answer came before the snapshot let go of the backup.
    expect(backUp).toBeEnabled();
    expect(backUp).toHaveAttribute('aria-disabled', 'true');
    rerender(<BackupsView snapshot={shown()} />);
    expect(backUp).not.toHaveAttribute('aria-disabled');
    expect(backUp).toHaveFocus();
  });

  it('waits while a game or another operation runs', async () => {
    const { calls } = setup(
      oneWorld,
      {},
      shown({
        activeOperation: { kind: 'launch', instanceId: instance.id, operationId: 'op-9' },
      }),
    );
    expect(await screen.findByText('My World', { selector: 'bdi' })).toBeVisible();
    expect(
      screen.getByText('Backups and restores wait until the running operation or game ends.'),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Back up' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Restore' })).toBeDisabled();
    expect(named('list_backups', calls)).toHaveLength(1);
  });

  it('does not say there is nothing before the list comes', () => {
    mockIPC((cmd) => (cmd === 'list_backups' ? new Promise(() => undefined) : null));
    render(<BackupsView snapshot={shown()} />);
    expect(screen.getByText('Looking for backups…')).toBeVisible();
    expect(screen.queryByText('No backups yet.')).toBeNull();
    expect(screen.queryByText('This installation has no worlds yet.')).toBeNull();
  });

  it('offers a copy only of a world, not of a whole installation', async () => {
    setup({ ...oneWorld, backups: [backup({ scope: { kind: 'installation' } })] });
    expect(await screen.findByText('Whole installation')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Restore' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Restore as a copy' })).toBeNull();
  });

  it('says what failed where it was pressed, until the next press', async () => {
    const other = instanceFixture({
      id: '2a2b2c2d-0000-4000-8000-000000000002',
      displayName: 'Creative',
    });
    setup(
      oneWorld,
      {
        create_backup: () => {
          throw new Error('The disk is full.');
        },
      },
      shown({ instances: [instance, other] }),
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Back up' }));
    const worlds = screen.getByRole('region', { name: 'Worlds' });
    const list = screen.getByRole('region', { name: /Backups of this installation/ });
    expect(await within(worlds).findByRole('alert')).toHaveTextContent('The disk is full.');
    expect(within(list).queryByRole('alert')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Restore as a copy' }));
    expect(await within(list).findByText(/Restored as the world/)).toBeVisible();
    expect(screen.queryByText('The disk is full.')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Back up' }));
    expect(await within(worlds).findByRole('alert')).toBeVisible();
    // Another installation: what was said about this one goes.
    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: other.id } });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('switches automatic backups after playing', async () => {
    const { calls } = setup({
      backups: [],
      worlds: [],
      storeBytes: 0,
      deletedBytes: [],
      deletedTotalBytes: 0,
    });
    fireEvent.click(
      await screen.findByRole('switch', { name: /Back up changed worlds after playing/ }),
    );
    await waitFor(() => {
      expect(named('set_app_settings', calls)[0]?.args).toMatchObject({
        request: { payload: { backupAfterPlay: true } },
      });
    });
  });

  it('keeps only a whole number of backups from 1 to 100', async () => {
    const { calls, rerender } = setup({
      backups: [],
      worlds: [],
      storeBytes: 0,
      deletedBytes: [],
      deletedTotalBytes: 0,
    });
    const field = await screen.findByRole('spinbutton', {
      name: 'Automatic backups kept per world',
    });
    const save = screen.getByRole('button', { name: 'Save' });
    for (const junk of ['0', '101', '1.5', '-3', '']) {
      fireEvent.change(field, { target: { value: junk } });
      fireEvent.click(save);
      expect(screen.getByRole('alert')).toHaveTextContent('Enter a whole number from 1 to 100.');
      expect(field).toHaveAttribute('aria-invalid', 'true');
      expect(field).toHaveAccessibleDescription(/Enter a whole number from 1 to 100\./);
    }
    // Saved again unchanged, it is said again: a new alert.
    const said = screen.getByRole('alert');
    fireEvent.click(save);
    expect(screen.getByRole('alert')).not.toBe(said);
    expect(named('set_app_settings', calls)).toHaveLength(0);

    fireEvent.change(field, { target: { value: '20' } });
    expect(screen.queryByRole('alert')).toBeNull();
    save.focus();
    fireEvent.click(save);
    await waitFor(() => {
      expect(named('set_app_settings', calls)[0]?.args).toMatchObject({
        request: { payload: { backupKeep: 20 } },
      });
    });
    // The snapshot brings the number saved; nothing is drawn again, so Save keeps the focus.
    rerender(
      <BackupsView
        snapshot={shown({ settings: { ...snapshotFixture().settings, backupKeep: 20 } })}
      />,
    );
    await waitFor(() => {
      expect(field).toHaveValue(20);
    });
    expect(save).toBeInTheDocument();
    expect(save).toHaveFocus();
  });

  it('tells the buttons of backups of one world apart by their date', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date(2026, 9, 17, 18, 0));
    setup({
      ...oneWorld,
      backups: [
        backup({ createdAt: new Date(2026, 9, 17, 12, 0).toISOString() }),
        backup({ id: 'older', createdAt: new Date(2026, 9, 16, 9, 30).toISOString() }),
      ],
    });
    const restores = await screen.findAllByRole('button', { name: 'Restore' });
    expect(restores[0]).toHaveAccessibleDescription(/My World.*Today, 12:00\sPM/u);
    expect(restores[1]).toHaveAccessibleDescription(/My World.*Yesterday, 9:30\sAM/u);
  });

  it('hands the focus on when the list after a delete comes after the snapshot', async () => {
    // The lists come when the test says: the one after the delete is overtaken.
    const lists: ((value: BackupOverview) => void)[] = [];
    let first = true;
    let finish: (value: number) => void = () => undefined;
    const both = { ...oneWorld, backups: [backup(), backup({ id: 'older' })] };
    const { rerender } = setup(both, {
      list_backups: () => {
        if (first) {
          first = false;
          return both;
        }
        return new Promise((resolve) => {
          lists.push(resolve);
        });
      },
      delete_backup: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    });
    const [remove] = await screen.findAllByRole('button', { name: 'Delete' });
    fireEvent.click(remove as HTMLElement);
    fireEvent.click(answer('Delete'));
    const ours = shown({
      activeOperation: { kind: 'backup', instanceId: instance.id, operationId: 'op-2' },
    });
    rerender(<BackupsView snapshot={ours} />);
    finish(1024 * 1024);
    await waitFor(() => {
      expect(lists).toHaveLength(1);
    });
    rerender(<BackupsView snapshot={shown()} />);
    await waitFor(() => {
      expect(lists).toHaveLength(2);
    });
    const left = { ...oneWorld, backups: [backup({ id: 'older' })] };
    lists[0]?.(left);
    lists[1]?.(left);
    const list = screen.getByRole('list', { name: /Backups of this installation/ });
    await waitFor(() => {
      expect(within(list).getAllByRole('listitem')).toHaveLength(1);
    });
    await waitFor(() => {
      expect(list).toHaveFocus();
    });
  });

  it('moves the focus only for its own press, not when a later list changes', async () => {
    let found: BackupOverview = { ...oneWorld, backups: [backup(), backup({ id: 'older' })] };
    const { rerender } = setup(found, {
      list_backups: () => found,
      delete_backup: () => {
        found = { ...found, backups: [backup({ id: 'older' })] };
        return 1024 * 1024;
      },
    });
    const [remove] = await screen.findAllByRole('button', { name: 'Delete' });
    fireEvent.click(remove as HTMLElement);
    fireEvent.click(answer('Delete'));
    const list = screen.getByRole('list', { name: /Backups of this installation/ });
    await waitFor(() => {
      expect(list).toHaveFocus();
    });
    // A mouse press elsewhere (macOS WebKit) leaves the focus on the page; then an
    // automatic backup after a game shows up.
    list.blur();
    found = { ...found, backups: [backup({ id: 'auto', reason: 'after_play' }), ...found.backups] };
    rerender(
      <BackupsView
        snapshot={shown({
          activeOperation: { kind: 'launch', instanceId: instance.id, operationId: 'op-4' },
        })}
      />,
    );
    rerender(<BackupsView snapshot={shown()} />);
    expect(await screen.findByText('After playing')).toBeVisible();
    expect(document.body).toHaveFocus();
  });

  it('keeps the focus on the screen when the installation brought back leaves the list', async () => {
    const gone = backup({
      id: 'whole',
      instanceId: 'b0b0b0b0-0000-4000-8000-00000000000b',
      instanceName: 'Old modpack',
      versionId: '1.20.1',
      scope: { kind: 'installation' },
      reason: 'before_delete',
    });
    let fail: (error: Error) => void = () => undefined;
    const { rerender } = setup(
      { backups: [gone], worlds: [], storeBytes: 0, deletedBytes: [], deletedTotalBytes: 0 },
      {
        restore_backup: () =>
          new Promise((_, reject) => {
            fail = reject;
          }),
      },
    );
    const bringBack = await screen.findByRole('button', { name: 'Bring back' });
    bringBack.focus();
    fireEvent.click(bringBack);
    // The installation is made again first: its backups leave the deleted ones.
    const back = instanceFixture({ id: gone.instanceId, displayName: 'Old modpack' });
    rerender(<BackupsView snapshot={shown({ instances: [instance, back] })} />);
    const title = screen.getByRole('heading', { name: 'Backups', level: 2 });
    await waitFor(() => {
      expect(title).toHaveFocus();
    });
    fail(new Error('The disk is full.'));
    expect(await screen.findByRole('alert')).toHaveTextContent('The disk is full.');
    expect(title).toHaveFocus();
  });

  it('gives the focus to the deleted ones when their question closes and its button waits', async () => {
    const { rerender } = setup({
      backups: [backup({ instanceId: 'gone', instanceName: 'Old modpack' })],
      worlds: [],
      storeBytes: 0,
      deletedBytes: [],
      deletedTotalBytes: 0,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Delete' }));
    expect(screen.getByRole('alertdialog')).toHaveFocus();
    // A game starts: the row's Delete is disabled and cannot take the focus back.
    rerender(
      <BackupsView
        snapshot={shown({
          activeOperation: { kind: 'launch', instanceId: instance.id, operationId: 'op-3' },
        })}
      />,
    );
    fireEvent.keyDown(screen.getByRole('alertdialog'), { key: 'Escape' });
    expect(screen.queryByRole('alertdialog')).toBeNull();
    await waitFor(() => {
      expect(screen.getByRole('list', { name: 'Deleted installations' })).toHaveFocus();
    });
  });

  it('says what deleting the backups of each deleted installation frees', async () => {
    const M = 1024 * 1024;
    const old = (id: string) =>
      backup({
        id,
        instanceId: 'gone',
        instanceName: 'Old modpack',
        scope: { kind: 'installation' },
        reason: 'before_delete',
      });
    let found: BackupOverview = {
      ...oneWorld,
      backups: [
        backup(),
        old('old-1'),
        old('old-2'),
        backup({ id: 'creative', instanceId: 'gone2', instanceName: 'Creative' }),
      ],
      deletedBytes: [
        { instanceId: 'gone', bytes: 3 * M },
        { instanceId: 'gone2', bytes: 0 },
      ],
      // More than the two lines: what the two share counts only together.
      deletedTotalBytes: 4 * M,
    };
    setup(found, {
      list_backups: () => found,
      delete_backup: () => {
        found = {
          ...found,
          backups: found.backups.filter((b) => b.id !== 'creative'),
          deletedBytes: found.deletedBytes.filter((d) => d.instanceId !== 'gone2'),
          deletedTotalBytes: 3 * M,
        };
        return 0;
      },
    });
    const panel = await screen.findByRole('region', { name: 'Deleted installations' });
    // A fact's value and the name beside it.
    const fact = (text: string) => {
      const value = within(panel).getByText(text);
      return { term: value.previousElementSibling?.textContent, value };
    };
    const old2 = fact('2 backups; deleting them all frees \u20683.0 MiB\u2069');
    expect(old2.term).toBe('Old modpack');
    // The size's number and unit stay on one line.
    expect(old2.value.textContent).toContain('3.0\u00a0MiB');
    expect(fact('1 backup; deleting it frees \u20680 bytes\u2069').term).toBe('Creative');
    expect(fact('3 backups; deleting them all frees \u20684.0 MiB\u2069').term).toBe(
      'All deleted installations',
    );
    // The live installation has none; a delete says what it freed, the lines are not
    // read out.
    expect(panel.querySelectorAll('dd')).toHaveLength(3);
    expect(old2.value.closest('[role=status],[role=alert],[aria-live]')).toBeNull();

    const list = within(panel).getByRole('list', { name: 'Deleted installations' });
    const creative = within(list)
      .getAllByRole('listitem')
      .find((row) => row.textContent.includes('Creative'));
    if (creative === undefined) throw new Error('no row for Creative');
    fireEvent.click(within(creative).getByRole('button', { name: 'Delete' }));
    fireEvent.click(answer('Delete'));
    await waitFor(() => {
      expect(within(panel).queryByText(/^1 backup;/)).toBeNull();
    });
    // One deleted installation left: its line is the total.
    expect(within(panel).getByText(/^2 backups;/)).toBeVisible();
    expect(within(panel).queryByText('All deleted installations')).toBeNull();
  });

  it('leaves out what the core did not count', async () => {
    const orphan = (id: string, instanceId: string, instanceName: string) =>
      backup({ id, instanceId, instanceName });
    setup({
      ...oneWorld,
      backups: [
        orphan('a', 'gone', 'Old modpack'),
        orphan('b', 'gone2', 'Creative'),
        orphan('c', 'gone3', 'Test'),
      ],
      // Deleted after the core counted: no line rather than a guess, and no total that
      // leaves it out.
      deletedBytes: [
        { instanceId: 'gone', bytes: 1024 },
        { instanceId: 'gone2', bytes: 2048 },
      ],
      deletedTotalBytes: 3072,
    });
    const panel = await screen.findByRole('region', { name: 'Deleted installations' });
    expect(within(panel).getByText(/^1 backup; deleting it frees \u20681\.0 KiB/)).toBeVisible();
    expect(within(panel).getByText(/^1 backup; deleting it frees \u20682\.0 KiB/)).toBeVisible();
    expect(panel.querySelectorAll('dd')).toHaveLength(2);
    expect(within(panel).queryByText('All deleted installations')).toBeNull();
  });

  it('gives no total that counts an installation brought back since', async () => {
    const back = instanceFixture({ id: 'gone3', displayName: 'Test' });
    setup(
      {
        ...oneWorld,
        backups: [
          backup({ id: 'a', instanceId: 'gone', instanceName: 'Old modpack' }),
          backup({ id: 'b', instanceId: 'gone2', instanceName: 'Creative' }),
          backup({ id: 'c', instanceId: 'gone3', instanceName: 'Test' }),
        ],
        // Counted while "Test" was still deleted.
        deletedBytes: [
          { instanceId: 'gone', bytes: 1024 },
          { instanceId: 'gone2', bytes: 2048 },
          { instanceId: 'gone3', bytes: 4096 },
        ],
        deletedTotalBytes: 7168,
      },
      {},
      shown({ instances: [instance, back] }),
    );
    const panel = await screen.findByRole('region', { name: 'Deleted installations' });
    await within(panel).findByText(/^1 backup; deleting it frees \u20682\.0 KiB/);
    expect(panel.querySelectorAll('dd')).toHaveLength(2);
    expect(within(panel).queryByText('All deleted installations')).toBeNull();
  });

  it('says "it" in Russian only for a single backup, not for 21', async () => {
    setLanguage(await loadLanguage('ru'));
    const space = (count: number) => strings.backups.deletedSpace({ count, size: '1 МиБ' });
    expect(space(1)).toBe('1 копия; её удаление освободит 1 МиБ');
    expect(space(21)).toBe('21 копия; удаление всех освободит 1 МиБ');
    expect(space(3)).toBe('3 копии; удаление всех освободит 1 МиБ');
    expect(space(5)).toBe('5 копий; удаление всех освободит 1 МиБ');
  });

  it('brings back a deleted installation from its backup', async () => {
    const { calls } = setup({
      backups: [
        backup({
          id: 'whole',
          instanceId: 'gone',
          instanceName: 'Old modpack',
          versionLabel: 'Minecraft 1.20.1 · Forge 47.4.10',
          versionId: '1.20.1',
          loader: { kind: 'forge', version: '1.20.1-47.4.10' },
          scope: { kind: 'installation' },
          reason: 'before_delete',
        }),
        backup({ id: 'world', instanceId: 'gone', instanceName: 'Old modpack' }),
      ],
      worlds: [],
      storeBytes: 0,
      deletedBytes: [],
      deletedTotalBytes: 0,
    });
    expect(await screen.findByRole('heading', { name: 'Deleted installations' })).toBeVisible();
    expect(screen.getByText('Before deleting')).toBeVisible();
    // Only the whole-installation backup can bring it back.
    const bringBack = screen.getAllByRole('button', { name: 'Bring back' });
    expect(bringBack).toHaveLength(1);
    fireEvent.click(bringBack[0] as HTMLElement);
    await waitFor(() => {
      expect(named('restore_backup', calls)[0]?.args).toMatchObject({
        request: { payload: { backupId: 'whole', asCopy: false } },
      });
    });
    const deleted = screen.getByRole('region', { name: 'Deleted installations' });
    expect(
      await within(deleted).findByText('"\u2068Old modpack\u2069" is back. Install it to play.'),
    ).toBeVisible();
  });
});
