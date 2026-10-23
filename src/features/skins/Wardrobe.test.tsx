import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type {
  AccountSummary,
  Closet,
  LauncherAccountView,
  PickedTexture,
  Wardrobe,
  WardrobeChange,
  WardrobeItem,
  WardrobeSet,
  WornTexture,
} from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { onOwnSkinChanged } from './ownSkin';
import { SkinsView } from './SkinsView';

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const id = (n: number) => n.toString(16).padStart(32, '0');
const hash = (n: number) => n.toString(16).padStart(64, '0');
const char = (code: number) => String.fromCodePoint(code);
/** Text set apart from the line around it, as the screen shows a file's or a nick's name. */
const isolated = (text: string) => `${char(0x2068)}${text}${char(0x2069)}`;

/** An answer the test gives when it chooses. */
function later() {
  let give: (value: unknown) => void = () => undefined;
  const answer = new Promise<unknown>((resolve) => {
    give = resolve;
  });
  return { answer, give };
}

function view(overrides: Partial<LauncherAccountView> = {}): LauncherAccountView {
  return {
    id: 'acc-1',
    nick: 'Steve_2',
    emailVerified: true,
    signingIn: false,
    needsSignIn: false,
    problem: null,
    ...overrides,
  };
}

function texture(n: number, overrides: Partial<WornTexture> = {}): WornTexture {
  return {
    hash: hash(n),
    slim: false,
    width: 64,
    height: 64,
    frames: 1,
    frameMs: [],
    ...overrides,
  };
}

function item(n: number, overrides: Partial<WardrobeItem> = {}): WardrobeItem {
  return {
    id: id(n),
    kind: 'skin',
    name: `Skin ${String(n)}`,
    favourite: false,
    created: 1_700_000_000 + n,
    worn: 0,
    texture: texture(n),
    ...overrides,
  };
}

function cape(n: number, overrides: Partial<WardrobeItem> = {}): WardrobeItem {
  return item(n, {
    kind: 'cape',
    name: `Cape ${String(n)}`,
    texture: texture(n, { height: 32 }),
    ...overrides,
  });
}

function set(n: number, overrides: Partial<WardrobeSet> = {}): WardrobeSet {
  return {
    id: id(100 + n),
    name: `Set ${String(n)}`,
    favourite: false,
    created: 1_700_000_000 + n,
    worn: 0,
    skin: texture(n),
    cape: texture(50 + n, { height: 32 }),
    ...overrides,
  };
}

function closet(overrides: Partial<Closet> = {}): Closet {
  return {
    worn: { skin: null, cape: null, uploads: { left: 7, resetSeconds: 600 } },
    items: [],
    sets: [],
    limits: { skins: 30, capes: 10, sets: 20 },
    ...overrides,
  };
}

function picked(overrides: Partial<PickedTexture> = {}): PickedTexture {
  return {
    kind: 'skin',
    name: 'neon_walk.png',
    width: 64,
    height: 256,
    frames: 4,
    frameMs: [],
    source: 'png',
    notes: [],
    bytes: 3000,
    preview: PNG,
    slim: null,
    ...overrides,
  };
}

type Answer = unknown;
type Call = { cmd: string; args: Record<string, unknown> };

/** The core: a string answer to a skins command is thrown, as the core rejects with a
 * problem code; a function is asked with the command's arguments. */
function backend(answers: Record<string, Answer>) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const given = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: given });
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    if (cmd in answers) {
      const answer = answers[cmd];
      const value =
        typeof answer === 'function'
          ? (answer as (a: Record<string, unknown>) => unknown)(given)
          : answer;
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      if (typeof value === 'string' && cmd !== 'skins_texture') throw value;
      return value;
    }
    if (cmd === 'skins_wardrobe') return closet();
    if (cmd === 'skins_texture') return PNG;
    return null;
  });
  return calls;
}

/** The Skins tab of a signed-in launcher account, with these answers. */
async function setup(answers: Record<string, Answer> = {}) {
  const calls = backend(answers);
  const base = snapshotFixture();
  const account: AccountSummary = { ...base.account, launcherAccount: view() };
  render(<SkinsView snapshot={{ ...base, account }} onOpenAccounts={vi.fn()} />);
  await screen.findByRole('tablist', { name: 'Wardrobe' });
  return calls;
}

const named = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd);
const changes = (calls: Call[]) =>
  named(calls, 'skins_wardrobe_change').map((c) => c.args.change as WardrobeChange);

/** The card of an entry, by its name. */
function card(name: string): HTMLElement {
  const li = screen.getByRole('button', { name: `Try on “${name}”` }).closest('li');
  if (li === null) throw new Error(`no card for ${name}`);
  return li;
}

/** Chooses an entry of a card's menu. */
function menu(name: string, entry: string) {
  fireEvent.click(within(card(name)).getByRole('button', { name: `Actions: ${name}` }));
  fireEvent.click(screen.getByRole('menuitem', { name: entry }));
}

function drop(files: File[]) {
  const target = screen.getByRole('button', { name: /^Upload a (skin|cape)/ }).closest('li');
  if (target === null) throw new Error('no drop target');
  const dataTransfer = { types: ['Files'], files, dropEffect: 'none' };
  fireEvent.dragOver(target, { dataTransfer });
  fireEvent.drop(target, { dataTransfer });
}

/** The screen's status region (notices have their own). */
function status(): HTMLElement {
  const region = screen.getAllByRole('status').find((element) => element.tagName === 'P');
  if (region === undefined) throw new Error('no status region');
  return region;
}

describe('the wardrobe', () => {
  it('shows what the account keeps, by kind, with nothing from the network', async () => {
    const calls = await setup({
      skins_wardrobe: closet({
        worn: { skin: texture(1), cape: null, uploads: { left: 7, resetSeconds: 600 } },
        items: [
          item(1),
          item(2, { texture: texture(2, { width: 128, height: 512, frames: 4, slim: true }) }),
          cape(3),
        ],
        sets: [set(1)],
      }),
    });
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent)).toEqual(['Skins 2', 'Capes 1', 'Sets 1']);
    expect(screen.getByRole('tab', { name: 'Skins 2' })).toHaveAttribute('aria-selected', 'true');
    expect(within(card('Skin 1')).getByText('Worn')).toBeVisible();
    expect(within(card('Skin 2')).getByText('HD ×2')).toBeVisible();
    expect(within(card('Skin 2')).getByText('4 frames')).toBeVisible();
    expect(within(card('Skin 2')).getByText('Slim arms')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Try on “Cape 3”' })).not.toBeInTheDocument();
    expect(screen.getByText(/Uploads left this hour: 7 of 10\./)).toBeVisible();
    expect(screen.getByText(/Wearing from the wardrobe is not an upload\./)).toBeVisible();
    expect(screen.getByText(/up to 30 skins, 10 capes and 20 sets/)).toBeVisible();
    expect(screen.getByText('Launcher account: Steve_2')).toBeVisible();
    // Pictures come from the core, by hash, each once: what is worn, the tab's cards in
    // their order, then the rest.
    await waitFor(() => {
      expect(named(calls, 'skins_texture')).toHaveLength(4);
    });
    expect(named(calls, 'skins_texture').map((c) => c.args.hash)).toEqual([
      hash(1),
      hash(2),
      hash(3),
      hash(51),
    ]);
    await waitFor(() => {
      expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
    });
    expect(document.querySelector('img[src^="http"]')).toBeNull();
    // Opening it asks for nothing it does not need.
    expect(
      calls.some((c) => ['skins_pick', 'skins_take', 'skins_upload', 'skins_keep'].includes(c.cmd)),
    ).toBe(false);
  });

  it('says what is empty, and when there is no room', async () => {
    await setup({
      skins_wardrobe: closet({ items: [cape(1)], limits: { skins: 30, capes: 1, sets: 20 } }),
    });
    expect(screen.getByText('No skins in the wardrobe yet.')).toBeVisible();
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 1' }));
    expect(screen.getByText(/no room for more capes/)).toBeVisible();
    fireEvent.click(screen.getByRole('tab', { name: 'Sets 0' }));
    expect(screen.getByText(/No sets yet/)).toBeVisible();
    // Nothing is worn: there is nothing to save as a set.
    expect(screen.getByRole('button', { name: 'Save what is worn as a set' })).toBeDisabled();
  });

  it('moves between tabs with the arrow keys, Home and End', async () => {
    await setup();
    const skins = screen.getByRole('tab', { name: 'Skins 0' });
    skins.focus();
    fireEvent.keyDown(skins, { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'Capes 0' })).toHaveFocus();
    expect(screen.getByRole('tab', { name: 'Capes 0' })).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Capes 0' }), { key: 'End' });
    expect(screen.getByRole('tab', { name: 'Sets 0' })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Sets 0' }), { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'Skins 0' })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Skins 0' }), { key: 'ArrowLeft' });
    expect(screen.getByRole('tab', { name: 'Sets 0' })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Sets 0' }), { key: 'Home' });
    expect(screen.getByRole('tab', { name: 'Skins 0' })).toHaveFocus();
    expect(screen.getByRole('tabpanel')).toHaveAccessibleName('Skins 0');
  });

  it('sorts by the order chosen, from a row of pills', async () => {
    await setup({
      skins_wardrobe: closet({
        items: [
          item(1, { name: 'beta', worn: 9 }),
          item(2, { name: 'Alpha', worn: 5 }),
          item(3, { name: 'gamma', favourite: true }),
        ],
      }),
    });
    const order = () =>
      screen
        .getAllByRole('button', { name: /^Try on/ })
        .map((b) => (b.getAttribute('aria-label') ?? '').slice(8, -1));
    const sorts = screen.getByRole('group', { name: 'Sort' });
    const pill = (name: string) => within(sorts).getByRole('button', { name });
    expect(pill('Recently worn')).toHaveAttribute('aria-pressed', 'true');
    expect(order()).toEqual(['beta', 'Alpha', 'gamma']);
    fireEvent.click(pill('Name'));
    expect(pill('Name')).toHaveAttribute('aria-pressed', 'true');
    expect(pill('Recently worn')).toHaveAttribute('aria-pressed', 'false');
    expect(order()).toEqual(['Alpha', 'beta', 'gamma']);
    fireEvent.click(pill('Newest'));
    expect(order()).toEqual(['gamma', 'Alpha', 'beta']);
    fireEvent.click(pill('Favourites'));
    expect(order()).toEqual(['gamma', 'beta', 'Alpha']);
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
  });

  it('names what has no name, and shows a name as text whatever it holds', async () => {
    await setup({
      skins_wardrobe: closet({
        items: [item(1, { name: '' }), item(2, { name: '<img src=x onerror=alert(1)>' })],
      }),
    });
    expect(screen.getByRole('button', { name: /^Try on “Skin of / })).toBeVisible();
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('says a picture did not load when the core gave something other than a PNG', async () => {
    await setup({
      skins_wardrobe: closet({ items: [item(1)] }),
      skins_texture: 'javascript:alert(1)',
    });
    expect(await within(card('Skin 1')).findByText('The picture did not load.')).toBeVisible();
    expect(document.querySelector('[src^="javascript"]')).toBeNull();
  });

  it('says why it could not be read, and tries again', async () => {
    let answer: unknown = 'network';
    backend({ skins_wardrobe: () => answer });
    const base = snapshotFixture();
    render(
      <SkinsView
        snapshot={{ ...base, account: { ...base.account, launcherAccount: view() } }}
        onOpenAccounts={vi.fn()}
      />,
    );
    expect(await screen.findByText(/cannot be reached/)).toBeVisible();
    answer = { items: 'junk' };
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    answer = closet();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByRole('tablist', { name: 'Wardrobe' })).toBeVisible();
  });

  it('says when the account must sign in again', async () => {
    backend({ skins_wardrobe: 'signed_out' });
    const base = snapshotFixture();
    render(
      <SkinsView
        snapshot={{ ...base, account: { ...base.account, launcherAccount: view() } }}
        onOpenAccounts={vi.fn()}
      />,
    );
    expect(await screen.findByText('The sign-in has ended. Sign in again.')).toBeVisible();
  });
});

describe('trying on', () => {
  const wardrobe = closet({
    worn: { skin: texture(9), cape: texture(8, { height: 32 }), uploads: null },
    items: [item(1), item(9, { name: 'Worn skin' }), cape(3), cape(8, { name: 'Worn cape' })],
    sets: [set(1), set(2, { name: 'Skin only', cape: null })],
  });

  it('tries a skin and a cape, then wears them from the wardrobe, never as an upload', async () => {
    const changed = vi.fn();
    const off = onOwnSkinChanged(changed);
    const calls = await setup({
      skins_wardrobe: wardrobe,
      skins_wardrobe_change: closet({
        ...wardrobe,
        worn: { ...wardrobe.worn, skin: texture(1), cape: texture(3, { height: 32 }) },
      }),
    });
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    expect(screen.getByRole('button', { name: 'Try on “Skin 1”' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 2' }));
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Cape 3”' }));
    const bar = screen.getByRole('region', { name: 'Fitting' });
    expect(within(bar).getByText('Trying on “Skin 1” and the cape “Cape 3”')).toBeVisible();
    expect(within(bar).getByText(/Only a fitting so far/)).toBeVisible();
    fireEvent.click(within(bar).getByRole('button', { name: 'Wear' }));
    await waitFor(() => {
      expect(status()).toHaveTextContent(/^Worn\. Players see it/);
    });
    expect(changes(calls)).toEqual([
      { action: 'wear', of: 'item', id: id(1) },
      { action: 'wear', of: 'item', id: id(3) },
    ]);
    expect(named(calls, 'skins_upload')).toHaveLength(0);
    expect(changed).toHaveBeenCalled();
    expect(screen.queryByRole('region', { name: 'Fitting' })).not.toBeInTheDocument();
    off();
  });

  it('tries nothing on for what is worn already, and the fitting can be cancelled', async () => {
    const calls = await setup({ skins_wardrobe: wardrobe });
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Worn skin”' }));
    expect(screen.queryByRole('region', { name: 'Fitting' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    const cancel = within(screen.getByRole('region', { name: 'Fitting' })).getByRole('button', {
      name: 'Cancel',
    });
    cancel.focus();
    fireEvent.click(cancel);
    expect(screen.queryByRole('region', { name: 'Fitting' })).not.toBeInTheDocument();
    // Its button is gone with it: the focus goes to the tab shown, not to the page.
    expect(screen.getByRole('tab', { name: 'Skins 2' })).toHaveFocus();
    expect(changes(calls)).toEqual([]);
  });

  it('tries a set, and says what comes off', async () => {
    const calls = await setup({ skins_wardrobe: wardrobe, skins_wardrobe_change: wardrobe });
    fireEvent.click(screen.getByRole('tab', { name: 'Sets 2' }));
    expect(within(card('Set 1')).getByText('Skin and cape')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin only”' }));
    const bar = screen.getByRole('region', { name: 'Fitting' });
    expect(within(bar).getByText('Trying on the set “Skin only”')).toBeVisible();
    expect(
      within(bar).getByText('The set has no cape: the cape worn now comes off.'),
    ).toBeVisible();
    fireEvent.click(within(bar).getByRole('button', { name: 'Wear' }));
    await waitFor(() => {
      expect(changes(calls)).toEqual([{ action: 'wear', of: 'set', id: id(102) }]);
    });
  });

  it('keeps the fitting when wearing is refused, and says why', async () => {
    await setup({ skins_wardrobe: wardrobe, skins_wardrobe_change: 'gone' });
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    fireEvent.click(
      within(screen.getByRole('region', { name: 'Fitting' })).getByRole('button', { name: 'Wear' }),
    );
    expect(await screen.findByText(/no longer in the wardrobe/)).toBeVisible();
    expect(screen.getByRole('region', { name: 'Fitting' })).toBeVisible();
  });
});

describe('a card', () => {
  it('marks a favourite', async () => {
    const one = item(1);
    const calls = await setup({
      skins_wardrobe: closet({ items: [one] }),
      skins_wardrobe_change: closet({ items: [{ ...one, favourite: true }] }),
    });
    const star = within(card('Skin 1')).getByRole('button', { name: 'Favourite' });
    expect(star).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(star);
    await waitFor(() => {
      expect(within(card('Skin 1')).getByRole('button', { name: 'Favourite' })).toHaveAttribute(
        'aria-pressed',
        'true',
      );
    });
    expect(changes(calls)).toEqual([
      { action: 'favourite', of: 'item', id: id(1), favourite: true },
    ]);
  });

  it('renames, refusing hidden characters before anything is sent', async () => {
    const one = item(1);
    const calls = await setup({
      skins_wardrobe: closet({ items: [one] }),
      skins_wardrobe_change: closet({ items: [{ ...one, name: 'Neon' }] }),
    });
    menu('Skin 1', 'Rename');
    const input = screen.getByRole('textbox', { name: 'Name in the wardrobe' });
    expect(input).toHaveFocus();
    expect(input).toHaveValue('Skin 1');
    for (const junk of [
      `Ne${char(0x202e)}on`,
      `a${char(0x200b)}b`,
      `a${char(0)}b`,
      'x'.repeat(40),
    ]) {
      fireEvent.change(input, { target: { value: junk } });
      expect(screen.getByText(/A name has up to 32 characters/)).toBeVisible();
      fireEvent.submit(input);
    }
    // Typing past the rule stops one character over, to say why.
    expect(input).toHaveValue('x'.repeat(33));
    expect(changes(calls)).toEqual([]);
    fireEvent.change(input, { target: { value: '  Neon  ' } });
    fireEvent.submit(input);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Try on “Neon”' })).toHaveFocus();
    });
    expect(changes(calls)).toEqual([{ action: 'rename', of: 'item', id: id(1), name: 'Neon' }]);
    expect(status()).toHaveTextContent('Renamed.');
  });

  it('closes the name with Escape and gives the focus back', async () => {
    await setup({ skins_wardrobe: closet({ items: [item(1)] }) });
    menu('Skin 1', 'Rename');
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Name in the wardrobe' }), {
      key: 'Escape',
    });
    expect(screen.queryByRole('textbox', { name: 'Name in the wardrobe' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try on “Skin 1”' })).toHaveFocus();
  });

  it('asks before removing, and the focus goes on', async () => {
    const calls = await setup({
      skins_wardrobe: closet({ items: [item(1), item(2)] }),
      skins_wardrobe_change: closet({ items: [item(2)] }),
    });
    menu('Skin 1', 'Remove from the wardrobe…');
    const question = screen.getByRole('alertdialog', {
      name: /Remove “Skin 1” from the wardrobe\?/,
    });
    expect(within(question).getByRole('button', { name: 'Cancel' })).toHaveFocus();
    fireEvent.click(within(question).getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('button', { name: 'Try on “Skin 1”' })).toHaveFocus();
    expect(changes(calls)).toEqual([]);
    menu('Skin 1', 'Remove from the wardrobe…');
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Remove' }),
    );
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Try on “Skin 1”' })).not.toBeInTheDocument();
    });
    expect(changes(calls)).toEqual([{ action: 'delete', of: 'item', id: id(1) }]);
    expect(status()).toHaveTextContent('Removed from the wardrobe.');
    // The card is gone: the focus goes to the tab shown, not to the page.
    expect(screen.getByRole('tab', { name: 'Skins 1' })).toHaveFocus();
  });

  it('wears from its menu, and takes off what is worn', async () => {
    const changed = vi.fn();
    const off = onOwnSkinChanged(changed);
    const one = item(1);
    const worn = closet({ worn: { skin: one.texture, cape: null, uploads: null }, items: [one] });
    const calls = await setup({
      skins_wardrobe: closet({ items: [one] }),
      skins_wardrobe_change: worn,
      skins_remove: { skin: null, cape: null, uploads: null } satisfies Wardrobe,
    });
    menu('Skin 1', 'Wear');
    await waitFor(() => {
      expect(within(card('Skin 1')).getByText('Worn')).toBeVisible();
    });
    expect(changes(calls)).toEqual([{ action: 'wear', of: 'item', id: id(1) }]);
    expect(changed).toHaveBeenCalledTimes(1);
    menu('Skin 1', 'Take off');
    await waitFor(() => {
      expect(status()).toHaveTextContent('The skin is removed.');
    });
    expect(named(calls, 'skins_remove').map((c) => c.args)).toEqual([{ kind: 'skin' }]);
    expect(within(card('Skin 1')).queryByText('Worn')).not.toBeInTheDocument();
    expect(changed).toHaveBeenCalledTimes(2);
    off();
  });
});

describe('what is worn', () => {
  it('keeps what is worn but not kept, or takes it off', async () => {
    const wornOnly = closet({ worn: { skin: texture(5), cape: null, uploads: null } });
    const calls = await setup({
      skins_wardrobe: wornOnly,
      skins_wardrobe_change: closet({ ...wornOnly, items: [item(5, { name: '' })] }),
    });
    expect(screen.getByText('Worn, not in the wardrobe')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Save to the wardrobe' }));
    await waitFor(() => {
      expect(status()).toHaveTextContent('Saved to the wardrobe.');
    });
    expect(changes(calls)).toEqual([{ action: 'keepWorn', kind: 'skin', name: '' }]);
    expect(screen.queryByText('Worn, not in the wardrobe')).not.toBeInTheDocument();
  });

  it('saves what is worn as a set and shows the sets', async () => {
    const worn = closet({
      worn: { skin: texture(5), cape: texture(6, { height: 32 }), uploads: null },
    });
    const calls = await setup({
      skins_wardrobe: worn,
      skins_wardrobe_change: closet({ ...worn, sets: [set(5, { name: '' })] }),
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save what is worn as a set' }));
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Sets 1' })).toHaveAttribute('aria-selected', 'true');
    });
    expect(changes(calls)).toEqual([{ action: 'keepSet', name: '' }]);
    expect(status()).toHaveTextContent('Saved as a set.');
    // The button may close once the sets are full: the focus is on the sets shown.
    expect(screen.getByRole('tab', { name: 'Sets 1' })).toHaveFocus();
  });

  it('offers cape or elytra on the back only while a cape is shown', async () => {
    await setup({ skins_wardrobe: closet({ items: [cape(6)] }) });
    // Nothing on the back: nothing to choose.
    expect(screen.queryByRole('radiogroup', { name: 'On the back' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 1' }));
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Cape 6”' }));
    const back = await screen.findByRole('radiogroup', { name: 'On the back' });
    // One choice, the wardrobe's: the stage does not draw its own as well.
    expect(screen.queryByRole('group', { name: 'On the back' })).not.toBeInTheDocument();
    fireEvent.click(within(back).getByRole('radio', { name: 'Elytra' }));
    expect(within(back).getByRole('radio', { name: 'Elytra' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Cape 6”' }));
    expect(screen.queryByRole('radiogroup', { name: 'On the back' })).not.toBeInTheDocument();
  });

  it('shows how players without the mod see the account', async () => {
    await setup({
      skins_wardrobe: closet({ worn: { skin: texture(5), cape: null, uploads: null } }),
    });
    fireEvent.click(screen.getByRole('switch', { name: 'Without the mod' }));
    expect(screen.getByText('Seen by players without the mod')).toBeVisible();
    expect(screen.getByText(/see the game's own skin and no cape/)).toBeVisible();
  });
});

describe('a new skin', () => {
  it('opens a picked file on its own screen and keeps and wears it', async () => {
    const changed = vi.fn();
    const off = onOwnSkinChanged(changed);
    const after = closet({
      worn: { skin: texture(7, { height: 256, frames: 4 }), cape: null, uploads: null },
    });
    const calls = await setup({
      skins_pick: picked({ frameMs: [100, 200, 300, 400] }),
      skins_upload: after.worn,
      skins_wardrobe_change: closet({ ...after, items: [item(7, { name: 'neon_walk' })] }),
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const heading = await screen.findByRole('heading', { name: 'New skin' });
    await waitFor(() => {
      expect(heading).toHaveFocus();
    });
    expect(named(calls, 'skins_pick')[0]?.args).toEqual({
      kind: 'skin',
      title: 'Choose a skin (PNG, GIF or APNG)',
    });
    const form = screen.getByRole('form', { name: 'New skin' });
    expect(screen.getByText(`${isolated('neon_walk.png')} · PNG 64×256`)).toBeVisible();
    expect(within(form).getByText('4 frames in a strip from top to bottom')).toBeVisible();
    expect(within(form).getByRole('textbox', { name: /Name in the wardrobe/ })).toHaveValue(
      'neon_walk',
    );
    expect(within(form).getByRole('spinbutton', { name: 'Frame 2 duration, ms' })).toHaveValue(200);
    expect(within(form).getByText(/Uploads left this hour: 7 of 10/)).toBeVisible();
    fireEvent.click(within(form).getByRole('radio', { name: 'Slim, 3 pixels wide' }));
    fireEvent.change(within(form).getByRole('spinbutton', { name: 'Frame 4 duration, ms' }), {
      target: { value: '250' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Save and wear' }));
    expect(await screen.findByRole('tablist', { name: 'Wardrobe' })).toBeVisible();
    expect(named(calls, 'skins_upload')[0]?.args).toEqual({
      kind: 'skin',
      slim: true,
      frameMs: [100, 200, 300, 250],
    });
    expect(changes(calls)).toEqual([{ action: 'keepWorn', kind: 'skin', name: 'neon_walk' }]);
    expect(named(calls, 'skins_keep')).toHaveLength(0);
    expect(status()).toHaveTextContent(/^Worn\./);
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Skins' })).toHaveFocus();
    });
    expect(screen.getByRole('button', { name: 'Try on “neon_walk”' })).toBeVisible();
    expect(changed).toHaveBeenCalledTimes(1);
    off();
  });

  it('keeps without wearing, with one duration for all', async () => {
    const changed = vi.fn();
    const off = onOwnSkinChanged(changed);
    const calls = await setup({
      skins_pick: picked({ frameMs: [120, 120, 120, 120] }),
      skins_keep: closet({ items: [item(7, { name: 'Neon' })] }),
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    expect(within(form).getByRole('switch', { name: 'One duration for all' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    fireEvent.change(within(form).getByRole('spinbutton', { name: 'Each frame, ms' }), {
      target: { value: '300' },
    });
    fireEvent.change(within(form).getByRole('textbox', { name: /Name in the wardrobe/ }), {
      target: { value: ' Neon ' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Save to the wardrobe' }));
    expect(await screen.findByRole('button', { name: 'Try on “Neon”' })).toBeVisible();
    expect(named(calls, 'skins_keep')[0]?.args).toEqual({
      kind: 'skin',
      slim: false,
      frameMs: [300],
      name: 'Neon',
    });
    expect(named(calls, 'skins_upload')).toHaveLength(0);
    expect(status()).toHaveTextContent('Saved to the wardrobe.');
    // Nothing worn changed.
    expect(changed).not.toHaveBeenCalled();
    off();
  });

  it('refuses durations and names outside the rule before anything is sent', async () => {
    const calls = await setup({ skins_pick: picked({ frameMs: [100, 150, 100, 100] }) });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    const input = within(form).getByRole('spinbutton', { name: 'Frame 1 duration, ms' });
    const wear = within(form).getByRole('button', { name: 'Save and wear' });
    const keep = within(form).getByRole('button', { name: 'Save to the wardrobe' });
    for (const junk of ['49', '10001', '', '-100', '1e3', '0x64']) {
      fireEvent.change(input, { target: { value: junk } });
      expect(wear).toBeDisabled();
      expect(keep).toBeDisabled();
    }
    fireEvent.change(input, { target: { value: '50' } });
    expect(wear).toBeEnabled();
    const name = within(form).getByRole('textbox', { name: /Name in the wardrobe/ });
    fireEvent.change(name, { target: { value: `a${char(0x2067)}b` } });
    expect(wear).toBeDisabled();
    expect(within(form).getByText(/A name has up to 32 characters/)).toBeVisible();
    fireEvent.submit(form);
    expect(calls.some((c) => c.cmd === 'skins_upload' || c.cmd === 'skins_keep')).toBe(false);
  });

  it('only wears when the wardrobe is full, and keeps nothing', async () => {
    const full = closet({ items: [item(1)], limits: { skins: 1, capes: 10, sets: 20 } });
    const calls = await setup({
      skins_wardrobe: full,
      skins_pick: picked({ frames: 1, height: 64 }),
      skins_upload: { skin: texture(7), cape: null, uploads: null },
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    expect(within(form).getByText(/this one can be worn but not kept/)).toBeVisible();
    expect(
      within(form).queryByRole('button', { name: 'Save to the wardrobe' }),
    ).not.toBeInTheDocument();
    fireEvent.click(within(form).getByRole('button', { name: 'Wear' }));
    await screen.findByRole('tablist', { name: 'Wardrobe' });
    expect(named(calls, 'skins_upload')[0]?.args).toEqual({
      kind: 'skin',
      slim: false,
      frameMs: null,
    });
    expect(changes(calls)).toEqual([]);
  });

  it('says when it was worn but could not be kept', async () => {
    await setup({
      skins_pick: picked({ frames: 1, height: 64 }),
      skins_upload: { skin: texture(7), cape: null, uploads: null },
      skins_wardrobe_change: 'wardrobe_full',
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save and wear' }));
    await waitFor(() => {
      expect(status()).toHaveTextContent('Worn, but not saved to the wardrobe.');
    });
    expect(screen.getByText(/The wardrobe is full/)).toBeVisible();
    expect(screen.getByText('Worn, not in the wardrobe')).toBeVisible();
  });

  it('says why the core or the service refused, and keeps the file for another try', async () => {
    let refusal: unknown = 'too_many_frames';
    await setup({
      skins_pick: () => refusal,
      skins_upload: () => refusal,
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    expect(await screen.findByText(/The animation is too long/)).toBeVisible();
    refusal = picked({ frames: 1, height: 64 });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    refusal = 'rate_limited';
    fireEvent.click(within(form).getByRole('button', { name: 'Save and wear' }));
    expect(
      await screen.findByText('Too many uploads: up to 10 an hour. Try again later.'),
    ).toBeVisible();
    expect(screen.getByRole('form', { name: 'New skin' })).toBeVisible();
    refusal = '<img src=x onerror=alert(1)>';
    fireEvent.click(within(form).getByRole('button', { name: 'Save and wear' }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('does not open what the core did not describe', async () => {
    await setup({ skins_pick: { ...picked(), preview: 'https://example.com/x.png' } });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(screen.queryByRole('form', { name: 'New skin' })).not.toBeInTheDocument();
  });

  it('goes back to the wardrobe without sending anything', async () => {
    const calls = await setup({ skins_pick: picked() });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    await screen.findByRole('form', { name: 'New skin' });
    fireEvent.click(screen.getByRole('button', { name: 'Wardrobe' }));
    expect(screen.getByRole('heading', { name: 'Skins' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    await screen.findByRole('form', { name: 'New skin' });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(await screen.findByRole('tablist', { name: 'Wardrobe' })).toBeVisible();
    expect(calls.some((c) => c.cmd === 'skins_upload' || c.cmd === 'skins_keep')).toBe(false);
  });

  it('says what importing a GIF changed, and steps through its frames', async () => {
    await setup({
      skins_pick: picked({
        source: 'gif',
        name: 'neon.gif',
        width: 256,
        height: 1024,
        frames: 4,
        frameMs: [100, 50, 50, 10_000],
        bytes: 400 * 1024,
        notes: [
          { kind: 'everyNth', from: 8, to: 4, step: 2 },
          { kind: 'clamped', frame: 1, was: 20, now: 50 },
          { kind: 'clamped', frame: 2, was: 30, now: 50 },
          { kind: 'clamped', frame: 3, was: 12_000, now: 10_000 },
          { kind: 'clamped', frame: 0, was: 49, now: 50 },
          { kind: 'clamped', frame: 9, was: 1, now: 50 },
        ],
      }),
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    expect(screen.getByText(`${isolated('neon.gif')} → PNG strip 256×1024`)).toBeVisible();
    for (const text of [
      'Frame 256×256, HD ×4',
      '400 KB of 2,048 KB',
      'The GIF became a strip of frames with its own durations.',
      'Frame 2 lasted 20 ms; the game shows it for 50 ms.',
      'Frame 4 lasted 12000 ms; the game shows it for 10000 ms.',
      '2 more frames were brought within 50–10000 ms.',
    ]) {
      expect(within(form).getByText(text)).toBeVisible();
    }
    const strip = screen.getByRole('group', { name: 'Frames' });
    fireEvent.click(within(strip).getByRole('button', { name: 'Pause' }));
    expect(within(strip).getByText('Frame 1 of 4 · 100 ms')).toBeVisible();
    fireEvent.click(within(strip).getByRole('button', { name: 'Previous frame' }));
    expect(within(strip).getByText('Frame 4 of 4 · 10,000 ms')).toBeVisible();
    fireEvent.click(within(strip).getByRole('button', { name: 'Hold frame 2' }));
    expect(within(strip).getByRole('button', { name: 'Hold frame 2' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(within(strip).getByText('Frame 2 of 4 · 50 ms')).toBeVisible();
  });

  it('takes a dropped file through the core, by its contents', async () => {
    const calls = await setup({
      skins_take: picked({ kind: 'cape', name: 'drop.gif', height: 64, frames: 2, source: 'gif' }),
    });
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 0' }));
    drop([new File([new Uint8Array([71, 73, 70, 56, 57, 97])], 'drop.gif')]);
    expect(await screen.findByRole('form', { name: 'New cape' })).toBeVisible();
    expect(named(calls, 'skins_take')[0]?.args).toEqual({
      kind: 'cape',
      name: 'drop.gif',
      data: 'R0lGODlh',
    });
    // A cape has no arms.
    expect(screen.queryByRole('radiogroup', { name: 'Arms' })).not.toBeInTheDocument();
  });

  it('refuses a dropped file of another kind or too heavy before reading it', async () => {
    const calls = await setup();
    drop([new File(['<svg onload=alert(1)>'], 'skin.svg')]);
    expect(
      await screen.findByText('This is not a PNG, GIF or APNG image, or the file is damaged.'),
    ).toBeVisible();
    const heavy = new File(['x'], 'big.png');
    Object.defineProperty(heavy, 'size', { value: 9 * 1024 * 1024 });
    drop([heavy]);
    expect(await screen.findByText(/The file is too large/)).toBeVisible();
    expect(calls.some((c) => c.cmd === 'skins_take')).toBe(false);
  });
});

describe('by nick', () => {
  it('takes what another player wears, as its owner wears it', async () => {
    const calls = await setup({
      skins_by_nick: picked({ name: 'Alex_99', frames: 1, height: 64, slim: true }),
    });
    const open = screen.getByRole('button', { name: 'Take a skin by nick' });
    fireEvent.click(open);
    expect(open).toHaveAttribute('aria-expanded', 'true');
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    const nick = within(form).getByRole('textbox', { name: /Player's nick/ });
    expect(nick).toHaveFocus();
    const take = within(form).getByRole('button', { name: 'Take' });
    for (const junk of ['ab', 'Алекс', 'a b c', '<script>', "x' OR 1=1", '../../etc']) {
      fireEvent.change(nick, { target: { value: junk } });
      expect(take).toBeDisabled();
      fireEvent.submit(form);
    }
    fireEvent.change(nick, { target: { value: 'a'.repeat(40) } });
    expect(nick).toHaveValue('a'.repeat(16));
    expect(named(calls, 'skins_by_nick')).toHaveLength(0);
    fireEvent.change(nick, { target: { value: 'Alex_99' } });
    fireEvent.click(take);
    const heading = await screen.findByRole('heading', { name: 'New skin' });
    await waitFor(() => {
      expect(heading).toHaveFocus();
    });
    expect(named(calls, 'skins_by_nick')[0]?.args).toEqual({ kind: 'skin', nick: 'Alex_99' });
    expect(screen.getByText(`Worn by ${isolated('Alex_99')}`)).toBeVisible();
    expect(screen.getByText('As its owner wears it.')).toBeVisible();
    expect(screen.getByRole('radio', { name: 'Slim, 3 pixels wide' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
  });

  it('says when nobody wears one, and closes with Escape', async () => {
    const calls = await setup({ skins_by_nick: 'not_found' });
    fireEvent.click(screen.getByRole('button', { name: 'Take a skin by nick' }));
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    fireEvent.click(within(form).getByRole('radio', { name: 'Cape' }));
    fireEvent.change(within(form).getByRole('textbox', { name: /Player's nick/ }), {
      target: { value: 'nobody' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Take' }));
    expect(await within(form).findByText(/Nobody with that nick/)).toBeVisible();
    expect(named(calls, 'skins_by_nick')[0]?.args).toEqual({ kind: 'cape', nick: 'nobody' });
    fireEvent.keyDown(within(form).getByRole('textbox', { name: /Player's nick/ }), {
      key: 'Escape',
    });
    expect(screen.queryByRole('form', { name: 'Take a skin by nick' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Take a skin by nick' })).toHaveFocus();
  });

  it('opens from the upload card for the tab shown', async () => {
    await setup();
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 0' }));
    fireEvent.click(screen.getByRole('button', { name: "Or take one by a friend's nick" }));
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    expect(within(form).getByRole('radio', { name: 'Cape' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
  });

  it('does not open what the core did not describe', async () => {
    await setup({ skins_by_nick: { ...picked(), kind: 'elytra' } });
    fireEvent.click(screen.getByRole('button', { name: 'Take a skin by nick' }));
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    fireEvent.change(within(form).getByRole('textbox', { name: /Player's nick/ }), {
      target: { value: 'Alex_99' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Take' }));
    expect(await within(form).findByText(/Something went wrong on the skins server/)).toBeVisible();
    expect(screen.queryByRole('heading', { name: 'New skin' })).not.toBeInTheDocument();
  });
});

describe('the skins tab', () => {
  function tab(launcherAccount: LauncherAccountView) {
    const calls = backend({});
    const base = snapshotFixture();
    const onOpenAccounts = vi.fn();
    render(
      <SkinsView
        snapshot={{ ...base, account: { ...base.account, launcherAccount } }}
        onOpenAccounts={onOpenAccounts}
      />,
    );
    return { onOpenAccounts, calls };
  }

  it('sends a player without an account to Accounts, asking nothing of the service', () => {
    const { onOpenAccounts, calls } = tab(view({ id: null, nick: null }));
    expect(screen.getByText(/Skins and capes belong to a launcher account/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Open Accounts' }));
    expect(onOpenAccounts).toHaveBeenCalled();
    expect(calls.some((c) => c.cmd.startsWith('skins_'))).toBe(false);
  });

  it('asks to sign in again when the sign-in ended', () => {
    const { calls } = tab(view({ needsSignIn: true, problem: 'sign_in_again' }));
    expect(screen.getByText(/sign-in has ended\. Sign in again in Accounts/)).toBeVisible();
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(calls.some((c) => c.cmd.startsWith('skins_'))).toBe(false);
  });
});

describe('while a request is on its way', () => {
  it('saves a new skin once, however often it is pressed, and waits to go back', async () => {
    const upload = later();
    const calls = await setup({
      skins_pick: picked({ frames: 1, height: 64 }),
      skins_upload: () => upload.answer,
      skins_wardrobe_change: closet({ items: [item(7)] }),
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    const wear = within(form).getByRole('button', { name: 'Save and wear' });
    fireEvent.click(wear);
    await waitFor(() => {
      expect(wear).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(wear);
    fireEvent.submit(form);
    fireEvent.click(within(form).getByRole('button', { name: 'Save to the wardrobe' }));
    // Going back now would hide a save that still happens.
    fireEvent.click(within(form).getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Wardrobe' }));
    expect(screen.getByRole('form', { name: 'New skin' })).toBeVisible();
    upload.give({ skin: texture(7), cape: null, uploads: null });
    expect(await screen.findByRole('tablist', { name: 'Wardrobe' })).toBeVisible();
    expect(named(calls, 'skins_upload')).toHaveLength(1);
    expect(named(calls, 'skins_keep')).toHaveLength(0);
    expect(changes(calls)).toEqual([{ action: 'keepWorn', kind: 'skin', name: 'neon_walk' }]);
  });

  it('keeps a new skin once', async () => {
    const keep = later();
    const calls = await setup({
      skins_pick: picked({ frames: 1, height: 64 }),
      skins_keep: () => keep.answer,
    });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    const save = within(form).getByRole('button', { name: 'Save to the wardrobe' });
    fireEvent.click(save);
    await waitFor(() => {
      expect(save).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(save);
    keep.give(closet({ items: [item(7)] }));
    await screen.findByRole('tablist', { name: 'Wardrobe' });
    expect(named(calls, 'skins_keep')).toHaveLength(1);
  });

  it('sends one change for a star, a menu and a removal pressed meanwhile', async () => {
    const answer = later();
    const calls = await setup({
      skins_wardrobe: closet({ items: [item(1), item(2)] }),
      skins_wardrobe_change: () => answer.answer,
    });
    const star = within(card('Skin 1')).getByRole('button', { name: 'Favourite' });
    fireEvent.click(star);
    await waitFor(() => {
      expect(star).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(star);
    menu('Skin 1', 'Wear');
    menu('Skin 2', 'Remove from the wardrobe…');
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Remove' }),
    );
    menu('Skin 2', 'Rename');
    fireEvent.change(screen.getByRole('textbox', { name: 'Name in the wardrobe' }), {
      target: { value: 'Neon' },
    });
    fireEvent.submit(screen.getByRole('textbox', { name: 'Name in the wardrobe' }));
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    fireEvent.click(
      within(screen.getByRole('region', { name: 'Fitting' })).getByRole('button', { name: 'Wear' }),
    );
    expect(changes(calls)).toEqual([
      { action: 'favourite', of: 'item', id: id(1), favourite: true },
    ]);
    answer.give(closet({ items: [{ ...item(1), favourite: true }, item(2)] }));
    await waitFor(() => {
      expect(star).not.toHaveAttribute('aria-disabled');
    });
    expect(changes(calls)).toHaveLength(1);
  });

  it('takes no file while one is being picked', async () => {
    const pick = later();
    const calls = await setup({ skins_pick: () => pick.answer });
    const upload = screen.getByRole('button', { name: /^Upload a skin/ });
    fireEvent.click(upload);
    await waitFor(() => {
      expect(upload).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(upload);
    const target = upload.closest('li');
    if (target === null) throw new Error('no drop target');
    // A file held over the card is not offered a place.
    fireEvent.dragOver(target, {
      dataTransfer: { types: ['Files'], files: [], dropEffect: 'none' },
    });
    expect(target).toHaveAttribute('data-over', 'false');
    drop([new File([new Uint8Array([137, 80, 78, 71])], 'other.png')]);
    pick.give(null);
    await waitFor(() => {
      expect(upload).not.toHaveAttribute('aria-disabled');
    });
    expect(named(calls, 'skins_pick')).toHaveLength(1);
    expect(named(calls, 'skins_take')).toHaveLength(0);
    // Closing the dialog is no problem.
    expect(screen.queryByText(/went wrong/)).not.toBeInTheDocument();
  });
});

describe('wearing what is tried on', () => {
  const wardrobe = closet({
    worn: { skin: texture(9), cape: texture(8, { height: 32 }), uploads: null },
    items: [item(1), item(9, { name: 'Worn skin' }), cape(3), cape(8, { name: 'Worn cape' })],
  });
  const skinOn = closet({ ...wardrobe, worn: { ...wardrobe.worn, skin: texture(1) } });

  async function tryBoth(second: unknown) {
    const changed = vi.fn();
    const off = onOwnSkinChanged(changed);
    let answers = 0;
    const calls = await setup({
      skins_wardrobe: wardrobe,
      skins_wardrobe_change: () => {
        answers += 1;
        return answers === 1 ? skinOn : second;
      },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 2' }));
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Cape 3”' }));
    fireEvent.click(
      within(screen.getByRole('region', { name: 'Fitting' })).getByRole('button', { name: 'Wear' }),
    );
    return { calls, changed, off };
  }

  it('keeps what was put on when the next piece is refused', async () => {
    const { calls, changed, off } = await tryBoth('gone');
    expect(await screen.findByText(/no longer in the wardrobe/)).toBeVisible();
    expect(changes(calls)).toHaveLength(2);
    // The skin is on; the cape is still tried on, to wear again.
    const bar = screen.getByRole('region', { name: 'Fitting' });
    expect(within(bar).getByText('Trying on the cape “Cape 3”')).toBeVisible();
    fireEvent.click(screen.getByRole('tab', { name: 'Skins 2' }));
    expect(within(card('Skin 1')).getByText('Worn')).toBeVisible();
    expect(changed).toHaveBeenCalledTimes(1);
    off();
  });

  it('keeps what was put on when the next answer is not a wardrobe', async () => {
    const { off } = await tryBoth({ items: 'junk' });
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(screen.getByRole('region', { name: 'Fitting' })).toBeVisible();
    fireEvent.click(screen.getByRole('tab', { name: 'Skins 2' }));
    expect(within(card('Skin 1')).getByText('Worn')).toBeVisible();
    off();
  });
});

describe('answers that are not what the core sends', () => {
  async function newSkin(answers: Record<string, Answer>) {
    const calls = await setup({ skins_pick: picked({ frames: 1, height: 64 }), ...answers });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    await screen.findByRole('form', { name: 'New skin' });
    return calls;
  }

  it('does not keep what is worn after an upload answered with junk', async () => {
    const calls = await newSkin({ skins_upload: { skin: { hash: 'x' }, cape: null } });
    fireEvent.click(screen.getByRole('button', { name: 'Save and wear' }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(screen.getByRole('form', { name: 'New skin' })).toBeVisible();
    expect(changes(calls)).toEqual([]);
  });

  it('says it was worn but not kept when keeping answered with junk', async () => {
    await newSkin({
      skins_upload: { skin: texture(7), cape: null, uploads: null },
      skins_wardrobe_change: { items: 'junk' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save and wear' }));
    await waitFor(() => {
      expect(status()).toHaveTextContent('Worn, but not saved to the wardrobe.');
    });
  });

  it('does not take junk for a kept skin', async () => {
    await newSkin({ skins_keep: { worn: null } });
    fireEvent.click(screen.getByRole('button', { name: 'Save to the wardrobe' }));
    expect(
      await screen.findByText('Something went wrong on the skins server. Try again later.'),
    ).toBeVisible();
    expect(screen.getByRole('form', { name: 'New skin' })).toBeVisible();
  });
});

describe('the character', () => {
  it('shows the game’s own skin while none is worn, and what is tried on', async () => {
    await setup({ skins_wardrobe: closet({ items: [item(1)] }) });
    expect(screen.getByText("No skin yet: the game's own is shown.")).toBeVisible();
    expect(screen.getByText('Worn now')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    expect(await screen.findByRole('img', { name: 'Skin from the front' })).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Skin from behind' })).toBeInTheDocument();
    expect(screen.queryByText('Worn now')).not.toBeInTheDocument();
    expect(screen.queryByText("No skin yet: the game's own is shown.")).not.toBeInTheDocument();
    // On the stage and on the card.
    expect(screen.getAllByText('Trying on')).toHaveLength(2);
  });

  it('keeps what is worn on the character until a tried-on picture comes', async () => {
    const picture = later();
    await setup({
      skins_wardrobe: closet({
        worn: { skin: texture(9), cape: null, uploads: null },
        items: [item(1), item(9)],
      }),
      skins_texture: ({ hash: asked }: Record<string, unknown>) =>
        asked === hash(1) ? picture.answer : PNG,
    });
    expect(await screen.findByRole('img', { name: 'Skin from the front' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try on “Skin 1”' }));
    expect(screen.getByRole('img', { name: 'Skin from the front' })).toBeInTheDocument();
    picture.give(PNG);
  });

  it('hides the cape for players without the mod', async () => {
    await setup({
      skins_wardrobe: closet({
        worn: { skin: texture(5), cape: texture(6, { height: 32 }), uploads: null },
      }),
    });
    expect(await screen.findByRole('img', { name: 'Cape from behind' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('switch', { name: 'Without the mod' }));
    expect(screen.queryByRole('img', { name: 'Cape from behind' })).not.toBeInTheDocument();
  });

  it('shows a picked file’s name as text, whatever it holds', async () => {
    await setup({ skins_pick: picked({ name: '<img src=x onerror=alert(1)>.png' }) });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    expect(
      await screen.findByText(`${isolated('<img src=x onerror=alert(1)>.png')} · PNG 64×256`),
    ).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });
});

describe('more of a card', () => {
  it('keeps the name form when the service refuses the name', async () => {
    const calls = await setup({
      skins_wardrobe: closet({ items: [item(1)] }),
      skins_wardrobe_change: 'bad_name',
    });
    menu('Skin 1', 'Rename');
    const input = screen.getByRole('textbox', { name: 'Name in the wardrobe' });
    fireEvent.change(input, { target: { value: 'Neon' } });
    fireEvent.submit(input);
    expect(await screen.findByText(/A name has up to 32 characters/)).toBeVisible();
    expect(screen.getByRole('textbox', { name: 'Name in the wardrobe' })).toHaveValue('Neon');
    expect(changes(calls)).toHaveLength(1);
  });

  it('closes the name form without asking when the name did not change', async () => {
    const calls = await setup({ skins_wardrobe: closet({ items: [item(1)] }) });
    menu('Skin 1', 'Rename');
    fireEvent.submit(screen.getByRole('textbox', { name: 'Name in the wardrobe' }));
    expect(screen.queryByRole('textbox', { name: 'Name in the wardrobe' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try on “Skin 1”' })).toHaveFocus();
    expect(changes(calls)).toEqual([]);
  });

  it('says why a name is refused, on the field', async () => {
    await setup({ skins_wardrobe: closet({ items: [item(1)] }), skins_pick: picked() });
    menu('Skin 1', 'Rename');
    const input = screen.getByRole('textbox', { name: 'Name in the wardrobe' });
    fireEvent.change(input, { target: { value: `a${char(0x200b)}b` } });
    expect(input).toHaveAccessibleDescription(/A name has up to 32 characters/);
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    const form = await screen.findByRole('form', { name: 'New skin' });
    const name = within(form).getByRole('textbox', { name: /Name in the wardrobe/ });
    expect(name).toHaveAccessibleDescription('Up to 32 characters, seen only by you.');
    fireEvent.change(name, { target: { value: `a${char(0x202e)}b` } });
    expect(name).toHaveAccessibleDescription(
      /^Up to 32 characters, seen only by you\. A name has up to 32 characters/,
    );
  });

  it('keeps the same status region from one view to the next, so its message is said', async () => {
    await setup({
      skins_pick: picked({ frames: 1, height: 64 }),
      skins_keep: closet({ items: [item(7)] }),
    });
    const region = status();
    fireEvent.click(screen.getByRole('button', { name: /^Upload a skin/ }));
    await screen.findByRole('form', { name: 'New skin' });
    expect(status()).toBe(region);
    fireEvent.click(screen.getByRole('button', { name: 'Save to the wardrobe' }));
    await screen.findByRole('tablist', { name: 'Wardrobe' });
    expect(status()).toBe(region);
    expect(region).toHaveTextContent('Saved to the wardrobe.');
  });

  it('moves between tabs with the arrow keys in the reading direction of Arabic', async () => {
    await setup();
    const tablist = screen.getByRole('tablist', { name: 'Wardrobe' });
    tablist.style.direction = 'rtl';
    const skins = screen.getByRole('tab', { name: 'Skins 0' });
    skins.focus();
    fireEvent.keyDown(skins, { key: 'ArrowLeft' });
    expect(screen.getByRole('tab', { name: 'Capes 0' })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Capes 0' }), { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'Skins 0' })).toHaveFocus();
  });
});

describe('by nick, more', () => {
  it('does not open an answer that came after the panel was closed', async () => {
    const answer = later();
    const calls = await setup({ skins_by_nick: () => answer.answer, skins_pick: picked() });
    fireEvent.click(screen.getByRole('button', { name: 'Take a skin by nick' }));
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    fireEvent.change(within(form).getByRole('textbox', { name: /Player's nick/ }), {
      target: { value: 'Alex_99' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Take' }));
    fireEvent.submit(form);
    // Meanwhile nothing else is picked: the core keeps the answer as the skin's file.
    const upload = screen.getByRole('button', { name: /^Upload a skin/ });
    await waitFor(() => {
      expect(upload).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(upload);
    fireEvent.click(within(form).getByRole('button', { name: 'Cancel' }));
    answer.give(picked({ name: 'Alex_99', slim: true }));
    await waitFor(() => {
      expect(upload).not.toHaveAttribute('aria-disabled');
    });
    expect(screen.queryByRole('heading', { name: 'New skin' })).not.toBeInTheDocument();
    expect(named(calls, 'skins_by_nick')).toHaveLength(1);
    expect(named(calls, 'skins_pick')).toHaveLength(0);
  });

  it('opens anew for the kind of the card it is opened from', async () => {
    await setup();
    fireEvent.click(screen.getByRole('button', { name: 'Take a skin by nick' }));
    fireEvent.change(screen.getByRole('textbox', { name: /Player's nick/ }), {
      target: { value: 'Alex_99' },
    });
    fireEvent.click(screen.getByRole('tab', { name: 'Capes 0' }));
    fireEvent.click(screen.getByRole('button', { name: "Or take one by a friend's nick" }));
    const form = screen.getByRole('form', { name: 'Take a skin by nick' });
    expect(within(form).getByRole('radio', { name: 'Cape' })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    expect(within(form).getByRole('textbox', { name: /Player's nick/ })).toHaveFocus();
  });

  it('opens a taken skin without an old problem', async () => {
    await setup({
      skins_wardrobe: closet({ items: [item(1)] }),
      skins_wardrobe_change: 'network',
      skins_by_nick: picked({ name: 'Alex_99', frames: 1, height: 64, slim: false }),
    });
    fireEvent.click(within(card('Skin 1')).getByRole('button', { name: 'Favourite' }));
    expect(await screen.findByText(/cannot be reached/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Take a skin by nick' }));
    fireEvent.change(screen.getByRole('textbox', { name: /Player's nick/ }), {
      target: { value: 'Alex_99' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Take' }));
    expect(await screen.findByRole('form', { name: 'New skin' })).toBeVisible();
    expect(screen.queryByText(/cannot be reached/)).not.toBeInTheDocument();
  });
});
