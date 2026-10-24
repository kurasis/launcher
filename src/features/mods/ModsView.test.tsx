import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  ContentView,
  DocNode,
  ModsCollection,
  ModsHit,
  ModsImportRequest,
  ModsPack,
  ModsPlan,
  ModsProject,
  ModsSearchPage,
  ModsVersion,
  ModsVersionPage,
  ModsVersionsQuery,
} from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { PATIENCE_MS } from './AddToCatalog';
import { cannotAdd, installKind } from './helpers';
import { ModsView, type ModsSection } from './ModsView';
import { RichText } from './RichText';

const PICTURE = `https://files-mods.lapi.sh/i/${'a'.repeat(64)}.png`;
const PROJECT = '00000000-0000-4000-8000-000000000001';
const VERSION = '00000000-0000-4000-8000-000000000010';

const fabric = instanceFixture({
  displayName: 'Modded',
  loader: { kind: 'fabric', version: '0.16.14' },
});

const page: ModsSearchPage = {
  total: 1,
  hits: [
    {
      id: PROJECT,
      kind: 'mod',
      slug: 'sodium',
      title: 'Sodium',
      summary: 'Fast rendering',
      icon: PICTURE,
      downloads: 1500,
      status: 'ready',
      updated: 1_760_000_000,
      author: 'jellysquid',
      sources: ['modrinth'],
      loaders: ['fabric', 'quilt'],
      categoryIds: ['mod.optimization'],
      cover: null,
    },
  ],
};

const project: ModsProject = {
  id: PROJECT,
  kind: 'mod',
  slug: 'sodium',
  title: 'Sodium',
  summary: 'Fast rendering',
  body: [
    {
      k: 'p',
      c: [
        { k: 'text', text: 'Read the ' },
        { k: 'a', href: 'https://example.com/wiki', c: [{ k: 'text', text: 'wiki' }] },
      ],
    },
  ],
  license: 'LGPL-3.0',
  icon: null,
  clientSide: 'required',
  serverSide: 'unsupported',
  status: 'ready',
  downloads: 1500,
  followers: 10,
  published: 1_700_000_000,
  updated: 1_760_000_000,
  goneAt: null,
  authors: [{ name: 'jellysquid', role: 'owner' }],
  categories: ['optimization'],
  categoryIds: ['mod.optimization'],
  sources: [{ source: 'modrinth', url: 'https://modrinth.com/mod/sodium' }],
  gallery: [],
};

const version: ModsVersion = {
  id: VERSION,
  projectId: PROJECT,
  number: '0.6.0',
  name: 'Sodium 0.6.0',
  changelog: [],
  channel: 'release',
  published: 1_760_000_000,
  downloads: 100,
  saved: 'none',
  games: ['1.21.1'],
  loaders: ['fabric'],
  files: [
    {
      name: 'sodium.jar',
      size: 1024,
      sha256: null,
      primary: true,
      url: null,
      held: false,
      checked: false,
    },
  ],
  dependencies: [],
  goneAt: null,
  source: null,
};

const plan: ModsPlan = {
  items: [
    {
      projectId: PROJECT,
      title: 'Sodium',
      kind: 'mod',
      icon: null,
      versionId: VERSION,
      number: '0.6.0',
      published: 1_760_000_000,
      fileName: 'sodium.jar',
      size: 1024,
      kept: false,
      dependency: false,
      path: 'mods/sodium.jar',
      replaces: null,
      present: false,
    },
  ],
  missing: [],
  conflicts: [],
  misfit: null,
  totalBytes: 1024,
  approval: null,
  turnOn: [],
  problems: [],
  instead: null,
};

const content: ContentView = {
  items: [
    {
      path: 'mods/sodium.jar',
      folder: 'mods',
      name: 'sodium.jar',
      world: null,
      enabled: true,
      size: 1024,
      directory: false,
      projectId: PROJECT,
      versionId: VERSION,
      title: 'Sodium',
      versionNumber: '0.6.0',
      icon: null,
      dependency: false,
      source: null,
      fromPack: false,
    },
    {
      path: 'mods/mine.jar.disabled',
      folder: 'mods',
      name: 'mine.jar',
      world: null,
      enabled: false,
      size: 2048,
      directory: false,
      projectId: null,
      versionId: null,
      title: null,
      versionNumber: null,
      icon: null,
      dependency: false,
      source: null,
      fromPack: false,
    },
  ],
  worlds: [],
  pack: null,
};

/** An installation with nothing from the catalog in it. */
const NOTHING: ContentView = { items: [], worlds: [], pack: null };
const fresh = { mods_content: () => NOTHING };

/** Game versions newest first, releases before snapshots, as the catalog sends them. */
function newestGame(a: string, b: string): number {
  const release = /^\d+(\.\d+)+$/;
  if (release.test(a) !== release.test(b)) return release.test(a) ? -1 : 1;
  return b.localeCompare(a, 'en', { numeric: true });
}

/** A page of versions as the core answers it, from all of a project's versions. */
function pageFrom(list: ModsVersion[], query: ModsVersionsQuery): ModsVersionPage {
  const matching = list.filter(
    (v) =>
      (query.game === null || v.games.includes(query.game)) &&
      (query.loader === null || v.loaders.includes(query.loader)) &&
      (query.channel === null || v.channel === query.channel),
  );
  const limit = query.limit ?? 50;
  return {
    total: matching.length,
    versions: matching.slice(query.offset, query.offset + limit),
    games: [...new Set(list.flatMap((v) => v.games))].sort(newestGame),
    loaders: [...new Set(list.flatMap((v) => v.loaders))].sort(),
  };
}

type Answers = Record<string, (args: Record<string, unknown>) => unknown>;

function setup(
  answers: Answers = {},
  start: ModsSection = 'mod',
  instanceId = fabric.id,
  instances = [fabric],
) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(payload);
    // Pages of whatever the test lists as the project's versions.
    if (cmd === 'mods_versions_page') {
      const listed = answers.mods_versions?.(payload) ?? [version];
      return Promise.resolve(listed as ModsVersion[] | Promise<ModsVersion[]>).then((list) =>
        pageFrom(list, payload.query as ModsVersionsQuery),
      );
    }
    switch (cmd) {
      case 'mods_search':
        return page;
      case 'mods_project':
        return project;
      case 'mods_versions':
        return [version];
      case 'mods_plan':
        return plan;
      case 'mods_install':
        return 1;
      case 'mods_needed_by':
        return { neededBy: [], unused: [] };
      case 'mods_content':
        return content;
      case 'mods_updates':
      case 'mods_identify':
      case 'mods_harmful':
      case 'mods_imports':
      case 'mods_similar':
        return [];
      case 'plugin:event|listen':
        return 1;
      default:
        return null;
    }
  });
  function Harness() {
    const [section, setSection] = useState<ModsSection>(start);
    const [instance, setInstance] = useState<string | null>(instanceId);
    return (
      <ModsView
        snapshot={snapshotFixture({ instances })}
        instanceId={instance}
        section={section}
        onSection={setSection}
        onInstance={setInstance}
      />
    );
  }
  render(<Harness />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: Record<string, unknown> }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('RichText', () => {
  it('draws only safe elements and our pictures', () => {
    const onLink = vi.fn();
    const nodes: DocNode[] = [
      { k: 'h1', c: [{ k: 'text', text: 'Title' }] },
      { k: 'img', href: PICTURE, text: 'ours' },
      { k: 'img', href: 'https://tracker.example/p.gif', text: 'theirs' },
      { k: 'a', href: 'javascript:alert(1)', c: [{ k: 'text', text: 'bad link' }] },
      { k: 'script', c: [{ k: 'text', text: 'alert(2)' }] },
      { k: 'a', href: 'https://example.com', c: [{ k: 'text', text: 'good link' }] },
    ];
    const { container } = render(<RichText nodes={nodes} onLink={onLink} />);
    expect(screen.getByRole('heading', { name: 'Title' })).toBeVisible();
    const images = container.querySelectorAll('img');
    expect(images).toHaveLength(1);
    expect(images[0]?.getAttribute('src')).toBe(PICTURE);
    expect(screen.getByText('theirs')).toBeVisible();
    expect(container.querySelector('script')).toBeNull();
    expect(screen.queryByRole('link', { name: 'bad link' })).toBeNull();
    expect(screen.getByText('bad link')).toBeVisible();
    fireEvent.click(screen.getByRole('link', { name: 'good link' }));
    expect(onLink).toHaveBeenCalledWith('https://example.com');
  });

  it('leaves out empty headings and paragraphs and puts headings under the panel’s', () => {
    const nodes: DocNode[] = [
      { k: 'h3', c: [] },
      { k: 'h3', c: [{ k: 'text', text: '  ' }] },
      { k: 'p', c: [{ k: 'text', text: '\n' }] },
      { k: 'h2', c: [{ k: 'text', text: 'Big' }] },
      { k: 'h3', c: [{ k: 'text', text: 'Graphics' }] },
      { k: 'h4', c: [{ k: 'text', text: 'Small' }] },
      { k: 'p', c: [{ k: 'img', href: PICTURE, text: 'only a picture' }] },
    ];
    const { container } = render(<RichText nodes={nodes} onLink={vi.fn()} />);
    expect(container.querySelectorAll('h3, h4, h5, h6')).toHaveLength(3);
    // The project's name is a level 2 heading and its panels' titles level 3.
    expect(screen.getByRole('heading', { name: 'Big', level: 5 })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Graphics', level: 6 })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Small', level: 6 })).toBeVisible();
    expect(container.querySelectorAll('p')).toHaveLength(1);
  });
});

describe('cannotAdd', () => {
  const quilt = instanceFixture({
    displayName: 'Quilted',
    loader: { kind: 'quilt', version: '0.27.0' },
  });
  const vanilla = instanceFixture({ displayName: 'Plain', loader: null });

  it('says why a version cannot go into the installation', () => {
    expect(cannotAdd('mod', version, null)).toBe('Choose an installation at the top first.');
    expect(cannotAdd('mod', version, fabric)).toBeNull();
    // Quilt runs Fabric's mods.
    expect(cannotAdd('mod', version, quilt)).toBeNull();
    expect(cannotAdd('mod', version, vanilla)).toMatch(/“Plain” has no mod loader/);
    const optifine = instanceFixture({
      displayName: 'Fast',
      loader: { kind: 'optifine', version: '1.21.1_HD_U_J1' },
    });
    expect(cannotAdd('mod', version, optifine)).toMatch(/“Fast” runs OptiFine on its own/);
    // Shaders go in.
    expect(cannotAdd('shader', version, optifine)).toBeNull();
    expect(cannotAdd('mod', { ...version, loaders: ['forge', 'neoforge'] }, fabric)).toBe(
      'This version is for Forge, NeoForge, and “Modded” runs Fabric.',
    );
    expect(
      cannotAdd(
        'mod',
        { ...version, games: ['1.20.1', '1.20.2', '1.20.3', '1.20.4', '1.20.5'] },
        fabric,
      ),
    ).toBe(
      'This version is for Minecraft 1.20.1, 1.20.2, 1.20.3, 1.20.4…, and “Modded” is on 1.21.1.',
    );
    expect(
      cannotAdd(
        'mod',
        { ...version, files: version.files.map((f) => ({ ...f, held: true })) },
        fabric,
      ),
    ).toMatch(/malware check/);
    expect(cannotAdd('mod', { ...version, files: [] }, fabric)).toBe('This version has no file.');
    // A mod for no loader (OptiFine without Forge).
    expect(cannotAdd('mod', { ...version, loaders: [] }, fabric)).toBe(
      'This version works with no mod loader, so it cannot be added to an installation.',
    );
    // Other kinds only warn about the game version, and need no loader.
    expect(
      cannotAdd('resourcepack', { ...version, games: ['1.8.9'], loaders: [] }, vanilla),
    ).toBeNull();
  });

  it('lets a mod’s data pack version into a world of any installation', () => {
    const datapack = { ...version, loaders: ['datapack'] };
    expect(installKind('mod', datapack)).toBe('datapack');
    expect(installKind('mod', { ...version, loaders: ['datapack', 'fabric'] })).toBe('mod');
    expect(installKind('mod', { ...version, loaders: [] })).toBe('mod');
    expect(installKind('resourcepack', datapack)).toBe('resourcepack');
    expect(cannotAdd(installKind('mod', datapack), datapack, vanilla)).toBeNull();
  });
});

describe('ModsView', () => {
  it('searches with the installation’s game version and loader', async () => {
    const calls = setup();
    expect(await screen.findByText('Sodium')).toBeVisible();
    expect(screen.getByText('by \u2068jellysquid\u2069')).toBeVisible();
    const query = named('mods_search', calls)[0]?.args.query as Record<string, unknown>;
    expect(query).toMatchObject({ kind: 'mod', game: '1.21.1', loader: 'fabric', offset: 0 });

    fireEvent.click(
      within(screen.getByRole('group', { name: 'Loader' })).getByRole('button', {
        name: 'Any loader',
      }),
    );
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({ loader: null });
  });

  it('opens a project and adds a version with what it needs', async () => {
    const calls = setup(fresh);
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    expect(await within(article).findByText(/Not saved in the catalog yet/)).toBeVisible();
    // The tile pressed is gone: the focus is on the project's name.
    await waitFor(() => {
      expect(within(article).getByRole('heading', { level: 2, name: 'Sodium' })).toHaveFocus();
    });
    // Pages of versions: all of them counted, then the ones for the installation's game.
    expect(named('mods_versions', calls)).toHaveLength(0);
    const queries = named('mods_versions_page', calls).map((c) => c.args);
    expect(queries).toContainEqual({
      project: PROJECT,
      query: { game: null, loader: null, channel: null, offset: 0, limit: 1 },
    });
    expect(queries).toContainEqual({
      project: PROJECT,
      query: { game: '1.21.1', loader: null, channel: null, offset: 0, limit: 100 },
    });
    expect(
      within(article).getByText('Latest for Minecraft \u20681.21.1\u2069 and Fabric. Updated', {
        exact: false,
      }),
    ).toBeVisible();

    fireEvent.click(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    );
    const dialog = await screen.findByRole('dialog', { name: 'Add Sodium to Modded' });
    expect(await within(dialog).findByText('mods/sodium.jar')).toBeVisible();
    expect(within(dialog).getByText(/1.0 KiB to download/)).toBeVisible();
    expect(within(dialog).getByText('Fetched from source')).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Install' }));
    expect(await within(dialog).findByText(/1 file is in place/)).toBeVisible();
    const install = named('mods_install', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(install.payload).toEqual({
      instanceId: fabric.id,
      version: VERSION,
      world: null,
      replaces: null,
    });
  });

  it('warns of what the mods’ files say and offers a version that runs', async () => {
    const OTHER = '00000000-0000-4000-8000-000000000099';
    const calls = setup({
      ...fresh,
      mods_plan: (args) =>
        (args.payload as { version: string }).version === OTHER
          ? plan
          : {
              ...plan,
              instead: { versionId: OTHER, number: '0.5.9' },
              problems: [
                {
                  kind: 'wrong',
                  titles: ['Sodium'],
                  id: 'fabricloader',
                  versions: ['>=0.17'],
                  found: '0.16.14',
                  from: null,
                  loader: null,
                },
                {
                  kind: 'missing',
                  titles: ['Sodium'],
                  id: 'cloth-config',
                  versions: ['>=15', '<12'],
                  found: null,
                  from: null,
                  loader: null,
                },
              ],
            },
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    fireEvent.click(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    );
    const dialog = await screen.findByRole('dialog', { name: 'Add Sodium to Modded' });
    const strip = (text: string | null) => (text ?? '').replace(/[\u2068\u2069]/g, '');
    const items = (await within(dialog).findAllByRole('listitem')).map((li) =>
      strip(li.textContent),
    );
    expect(items).toContain('Sodium needs Fabric Loader >=0.17; the installation has 0.16.14.');
    expect(items).toContain(
      'Sodium needs cloth-config >=15 or <12, which is not in the installation.',
    );
    // A warning, not a block.
    expect(within(dialog).getByRole('button', { name: 'Install' })).toBeEnabled();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Take version 0.5.9' }));
    await waitFor(() => {
      expect(within(dialog).queryByRole('button', { name: 'Take version 0.5.9' })).toBeNull();
    });
    expect(within(dialog).queryByText(/Fabric Loader/)).toBeNull();
    fireEvent.click(await within(dialog).findByRole('button', { name: 'Install' }));
    expect(await within(dialog).findByText(/1 file is in place/)).toBeVisible();
    const install = named('mods_install', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(install.payload).toMatchObject({ version: OTHER });
  });

  it('says why no version can be added', async () => {
    setup({ ...fresh, mods_versions: () => [{ ...version, loaders: ['forge'] }] });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    const add = await within(article).findByRole('button', { name: 'Add to installation' });
    expect(add).toBeDisabled();
    const reason = 'This version is for Forge, and “Modded” runs Fabric.';
    expect(add).toHaveAttribute('title', reason);
    // A disabled button gets no hover everywhere: its wrapper says it too, and the
    // line under it in words.
    expect(add.parentElement).toHaveAttribute('title', reason);
    expect(add).toHaveAccessibleDescription(reason);
    expect(within(article).getByText(reason)).toBeVisible();
    // The row says it too, to a screen reader, instead of a button.
    const row = (await within(article).findByRole('rowheader', { name: /0\.6\.0/ })).closest('tr');
    expect(row).toHaveTextContent(`Not for this installation: ${reason}`);
    expect(within(row as HTMLElement).queryByRole('button', { name: /Add version/ })).toBeNull();
  });

  it('adds a map as a new world', async () => {
    const calls = setup({
      ...fresh,
      mods_project: () => ({ ...project, kind: 'world', title: 'Sky' }),
      mods_versions: () => [
        {
          ...version,
          loaders: [],
          files: [{ name: 'sky.zip', size: 2048, sha256: null, primary: true, url: null }],
        },
      ],
      mods_world_plan: () => ({ name: 'Sky (2)', size: 2048, kept: false, approval: null }),
      mods_install_world: () => 'Sky (2)',
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sky' });
    fireEvent.click(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    );
    const dialog = await screen.findByRole('dialog', { name: 'Add the map Sky to Modded' });
    expect(await within(dialog).findByText(/new world, “Sky \(2\)” \(2.0 KiB\)/)).toBeVisible();
    expect(within(dialog).getByText(/fetches the map from its source/)).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add the map' }));
    expect(await within(dialog).findByText(/The map is now the world “Sky \(2\)”/)).toBeVisible();
    expect(named('mods_world_plan', calls)[0]?.args).toEqual({
      payload: { instanceId: fabric.id, version: VERSION },
    });
    const install = named('mods_install_world', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(install.payload).toEqual({ instanceId: fabric.id, version: VERSION });
    expect(named('mods_install', calls)).toHaveLength(0);
  });

  it('lists the versions made for the installation first, and filters them', async () => {
    const v = (n: number, games: string[], loaders: string[], extra: Partial<ModsVersion>) => ({
      ...version,
      id: `00000000-0000-4000-8000-0000000000${String(20 + n)}`,
      number: `0.${String(n)}`,
      name: `Sodium 0.${String(n)}`,
      published: 1_760_000_000 - n,
      games,
      loaders,
      ...extra,
    });
    const calls = setup({
      ...fresh,
      mods_versions: () => [
        v(1, ['1.20.1'], ['fabric'], {}),
        v(2, ['1.21.1'], ['forge'], { channel: 'beta' }),
        v(3, ['1.21.1', '1.21'], ['fabric', 'quilt'], { saved: 'all' }),
      ],
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    // The panel takes the place of its placeholder once the versions are counted.
    const panel = () => within(article).getByRole('region', { name: 'Versions' });
    const numbers = () =>
      within(panel())
        .queryAllByRole('rowheader')
        .map((cell) => cell.textContent);
    // What fits first, then the rest, newest first; the rest say why they do not fit.
    await waitFor(() => {
      expect(numbers()).toEqual(['0.3', '0.1', '0.2']);
    });
    const versions = panel();
    expect(within(versions).getByRole('status')).toHaveTextContent('3 versions, 1 fits');
    expect(within(versions).getByText('In the catalog')).toBeVisible();
    expect(
      within(versions).getByRole('button', { name: 'Add version \u20680.3\u2069' }),
    ).toBeVisible();
    expect(within(versions).queryByRole('button', { name: /Add version \u20680\.1/ })).toBeNull();
    expect(within(versions).getAllByText(/^Not for this installation: /)).toHaveLength(2);
    // The card adds the one that fits.
    expect(
      within(article).getByRole('button', { name: 'Add \u20680.3\u2069 to installation' }),
    ).toBeEnabled();
    // "Only for" leaves out the rest, and gives them back.
    const only = within(versions).getByRole('switch', { name: 'Only for “\u2068Modded\u2069”' });
    fireEvent.click(only);
    expect(numbers()).toEqual(['0.3']);
    fireEvent.click(only);
    expect(numbers()).toEqual(['0.3', '0.1', '0.2']);

    const game = within(versions).getByLabelText('Game version');
    expect(
      within(game)
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['Any game version', '1.21.1', '1.21', '1.20.1']);
    const loaders = within(versions).getByRole('group', { name: 'Loader' });
    expect(
      within(loaders)
        .getAllByRole('button')
        .map((b) => b.textContent),
    ).toEqual(['Any loader', 'Fabric', 'Forge', 'Quilt']);

    fireEvent.change(game, { target: { value: '1.20.1' } });
    await waitFor(() => {
      expect(numbers()).toEqual(['0.1']);
    });
    expect(named('mods_versions_page', calls).at(-1)?.args.query).toEqual({
      game: '1.20.1',
      loader: null,
      channel: null,
      offset: 0,
      limit: 50,
    });
    expect(within(versions).getByRole('status')).toHaveTextContent('1 version, 0 fit');
    fireEvent.click(
      within(versions).getByRole('switch', { name: 'Only for “\u2068Modded\u2069”' }),
    );
    expect(within(versions).getByText('No version here fits “\u2068Modded\u2069”.')).toBeVisible();
    fireEvent.click(within(versions).getByRole('button', { name: 'Show all' }));
    await waitFor(() => {
      expect(numbers()).toEqual(['0.3', '0.1', '0.2']);
    });
    expect(within(versions).getByRole('switch')).not.toBeChecked();

    fireEvent.click(
      within(within(versions).getByRole('group', { name: 'Type' })).getByRole('button', {
        name: 'Alpha',
      }),
    );
    expect(await within(versions).findByText('No version matches the filters.')).toBeVisible();
    fireEvent.click(within(versions).getByRole('button', { name: 'Show all' }));
    await waitFor(() => {
      expect(numbers()).toHaveLength(3);
    });
  });

  it('does not install while something is missing or in the way', async () => {
    setup({
      ...fresh,
      mods_plan: () => ({
        ...plan,
        missing: [{ projectId: null, title: 'Fabric API' }],
        conflicts: [{ path: 'mods/sodium.jar', reason: 'name_taken', title: null }],
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    fireEvent.click(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findByText('Fabric API')).toBeVisible();
    expect(within(dialog).getByText(/mods\/sodium.jar is already in the folder/)).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Install' })).toBeDisabled();
  });

  it('says when a metered connection asks first', async () => {
    setup({
      ...fresh,
      mods_plan: () => ({
        ...plan,
        approval: { bytes: 1024, files: 1, reason: 'large', monthBytes: 0, monthlyLimitMib: 0 },
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    fireEvent.click(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findByText('Download 1.0 KiB?')).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Install' })).toBeEnabled();
  });

  it('asks before opening a link from a description', async () => {
    const calls = setup();
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    fireEvent.click(await screen.findByRole('link', { name: 'wiki' }));
    const dialog = screen.getByRole('dialog', { name: 'Open this link?' });
    expect(within(dialog).getByText('https://example.com/wiki')).toBeVisible();
    expect(named('mods_open_link', calls)).toHaveLength(0);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Open in browser' }));
    await waitFor(() => {
      expect(named('mods_open_link', calls)[0]?.args).toEqual({ url: 'https://example.com/wiki' });
    });
  });

  it('lists, turns off and removes what an installation holds', async () => {
    const calls = setup({}, 'installed');
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(within(mods).getByText('Sodium')).toBeVisible();
    expect(within(mods).getByText('Added by hand')).toBeVisible();
    expect(within(mods).getByText('Off')).toBeVisible();

    fireEvent.click(within(mods).getByRole('switch', { name: '\u2068Sodium\u2069 is on' }));
    await waitFor(() => {
      expect(named('mods_set_enabled', calls)).toHaveLength(1);
    });
    const toggle = named('mods_set_enabled', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(toggle.payload).toEqual({
      instanceId: fabric.id,
      path: 'mods/sodium.jar',
      enabled: false,
    });

    const removeButtons = within(mods).getAllByRole('button', { name: 'Remove' });
    fireEvent.click(removeButtons[1] as HTMLElement);
    expect(named('mods_remove', calls)).toHaveLength(0);
    fireEvent.click(await within(mods).findByRole('button', { name: 'Remove the file' }));
    await waitFor(() => {
      expect(named('mods_remove', calls)).toHaveLength(1);
    });
    const removed = named('mods_remove', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(removed.payload).toEqual({ instanceId: fabric.id, path: 'mods/mine.jar.disabled' });
  });

  it('names a modpack’s files as the catalog knows them, without a check', async () => {
    const calls = setup(
      {
        mods_content: () => ({
          ...content,
          items: content.items.map((i, n) =>
            n === 1 ? { ...i, path: 'mods/mine.jar', enabled: true, fromPack: true } : i,
          ),
        }),
        mods_identify: () => [
          {
            path: 'mods/mine.jar',
            projectId: '00000000-0000-4000-8000-000000000002',
            versionId: '00000000-0000-4000-8000-000000000020',
            title: 'Fabric API',
            icon: PICTURE,
            versionNumber: '0.100.0',
          },
        ],
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(await within(mods).findByText('Fabric API')).toBeVisible();
    expect(within(mods).getByText('0.100.0')).toBeVisible();
    expect(within(mods).getByText('From the modpack')).toBeVisible();
    expect(
      within(mods).getByRole('switch', { name: '\u2068Fabric API\u2069 is on' }),
    ).toBeVisible();
    expect(mods.querySelector(`img[src="${PICTURE}"]`)).not.toBeNull();
    expect(within(mods).getAllByRole('button', { name: 'In the catalog' })).toHaveLength(2);
    expect(named('mods_identify', calls)[0]?.args).toEqual({ instanceId: fabric.id });
    // Asked without the player pressing anything.
    expect(named('mods_updates', calls)).toHaveLength(0);
  });

  it('keeps file names when the catalog cannot be asked', async () => {
    setup(
      {
        mods_identify: () => Promise.reject(new Error('offline')),
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(within(mods).getByText('mine.jar')).toBeVisible();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('says where a file came from, the version a file by hand gives itself, and what the jars say', async () => {
    const calls = setup(
      {
        mods_content: () => ({
          ...content,
          items: content.items.map((i, n) => (n === 0 ? { ...i, source: 'modrinth' } : i)),
        }),
        mods_folder_check: () => ({
          problems: [
            {
              kind: 'missing',
              titles: ['mine.jar'],
              id: 'cloth-config',
              versions: [],
              found: null,
              from: null,
              loader: null,
            },
          ],
          versions: [{ path: 'mods/mine.jar.disabled', version: '2.4.1' }],
        }),
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(
      within(mods).getByText(
        (_, element) =>
          element?.tagName === 'SPAN' &&
          element.textContent.startsWith('\u2068Modrinth\u2069 · mods/sodium.jar'),
      ),
    ).toBeVisible();
    expect(await within(mods).findByText('2.4.1')).toBeVisible();
    // The catalog's own version wins over the jar's.
    expect(within(mods).getByText('0.6.0')).toBeVisible();
    expect(
      screen.getByText('The mods’ own files say the game may not start with this:'),
    ).toBeVisible();
    expect(screen.getByText(/mine\.jar.* needs .*cloth-config/)).toBeVisible();
    expect(named('mods_folder_check', calls)[0]?.args).toEqual({ instanceId: fabric.id });
  });

  it('keeps the offers when a file is turned off, at its new path', async () => {
    let off = false;
    const calls = setup(
      {
        mods_content: () => ({
          ...content,
          items: content.items.map((i, n) =>
            n === 0 && off ? { ...i, path: 'mods/sodium.jar.disabled', enabled: false } : i,
          ),
        }),
        mods_set_enabled: () => {
          off = true;
          return null;
        },
        mods_updates: () => [
          {
            path: 'mods/sodium.jar',
            projectId: PROJECT,
            fromVersionId: VERSION,
            toVersionId: '00000000-0000-4000-8000-000000000011',
            toNumber: '0.6.1',
          },
        ],
      },
      'installed',
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText('Update: \u20680.6.1\u2069')).toBeVisible();
    const mods = screen.getByRole('region', { name: /^Mods \d/ });
    fireEvent.click(within(mods).getByRole('switch', { name: '\u2068Sodium\u2069 is on' }));
    await waitFor(() => {
      expect(within(mods).getAllByText('Off')).toHaveLength(2);
    });
    expect(screen.getByText('Update: \u20680.6.1\u2069')).toBeVisible();
    expect(named('mods_updates', calls)).toHaveLength(1);
    fireEvent.click(within(mods).getByRole('button', { name: 'Update' }));
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toMatchObject({
      replaces: 'mods/sodium.jar.disabled',
    });
  });

  it('warns about files the catalog marks as malware and removes one', async () => {
    const calls = setup({ mods_harmful: () => ['mods/mine.jar.disabled'] }, 'installed');
    expect(await screen.findByText('Harmful files found')).toBeVisible();
    expect(screen.getByText(/marked these files as malware/)).toBeVisible();
    // The warning comes before the list, its button first.
    const remove = screen.getAllByRole('button', { name: 'Remove' })[0] as HTMLElement;
    fireEvent.click(remove);
    await waitFor(() => {
      expect(named('mods_remove', calls)).toHaveLength(1);
    });
    const removed = named('mods_remove', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(removed.payload).toEqual({ instanceId: fabric.id, path: 'mods/mine.jar.disabled' });
  });

  it('does not offer a version the malware check holds back', async () => {
    const held: ModsVersion = {
      ...version,
      saved: 'all',
      files: [
        {
          name: 'sodium.jar',
          size: 1024,
          sha256: 'a'.repeat(64),
          primary: true,
          url: null,
          held: true,
          checked: false,
        },
      ],
    };
    setup({ mods_versions: () => [held] });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    expect(await within(article).findByText(/malware check is holding/)).toBeVisible();
    expect(within(article).queryByText('In the catalog')).toBeNull();
    expect(within(article).getByRole('button', { name: 'Add to installation' })).toBeDisabled();
  });

  it('says which mods need one before it is turned off or removed', async () => {
    const calls = setup(
      {
        mods_needed_by: () => ({
          neededBy: ['Iris'],
          unused: [{ path: 'mods/lib.jar', title: 'Library' }],
        }),
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    fireEvent.click(within(mods).getByRole('switch', { name: '\u2068Sodium\u2069 is on' }));
    expect(
      await within(mods).findByText('Needed by \u2068Iris\u2069; they will not start without it.'),
    ).toBeVisible();
    expect(named('mods_set_enabled', calls)).toHaveLength(0);
    fireEvent.click(within(mods).getByRole('button', { name: 'Cancel' }));
    expect(within(mods).queryByText(/Needed by/)).toBeNull();
    fireEvent.click(within(mods).getByRole('switch', { name: '\u2068Sodium\u2069 is on' }));
    fireEvent.click(await within(mods).findByRole('button', { name: 'Turn off anyway' }));
    await waitFor(() => {
      expect(named('mods_set_enabled', calls)).toHaveLength(1);
    });

    // Removing: what only it needed goes too, unless the player says not.
    fireEvent.click(within(mods).getAllByRole('button', { name: 'Remove' })[0] as HTMLElement);
    const also = await within(mods).findByRole('checkbox', {
      name: 'Also remove what only it needed: \u2068Library\u2069',
    });
    expect(also).toBeChecked();
    fireEvent.click(within(mods).getByRole('button', { name: 'Remove the file' }));
    await waitFor(() => {
      expect(named('mods_remove', calls)).toHaveLength(2);
    });
    const paths = named('mods_remove', calls).map(
      (c) => (c.args.request as { payload: { path: string } }).payload.path,
    );
    expect(paths).toEqual(['mods/sodium.jar', 'mods/lib.jar']);
  });

  it('offers updates of what the catalog knows', async () => {
    const calls = setup(
      {
        mods_updates: () => [
          {
            path: 'mods/sodium.jar',
            projectId: PROJECT,
            fromVersionId: VERSION,
            toVersionId: '00000000-0000-4000-8000-000000000011',
            toNumber: '0.6.1',
          },
        ],
      },
      'installed',
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText('Update: \u20680.6.1\u2069')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Update' }));
    await screen.findByRole('dialog');
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toMatchObject({
      version: '00000000-0000-4000-8000-000000000011',
      replaces: 'mods/sodium.jar',
    });
  });

  it('says what a check found at the toolbar, and updates everything with the focus kept', async () => {
    const offers = [
      {
        path: 'mods/sodium.jar',
        projectId: PROJECT,
        fromVersionId: VERSION,
        toVersionId: '00000000-0000-4000-8000-000000000011',
        toNumber: '0.6.1',
      },
      {
        path: 'mods/mine.jar.disabled',
        projectId: PROJECT,
        fromVersionId: VERSION,
        toVersionId: '00000000-0000-4000-8000-000000000012',
        toNumber: '2.0',
      },
    ];
    // The first check and the first install wait for the test; so does the check after
    // "Update all", which then finds nothing.
    const answers: ((found: typeof offers) => void)[] = [];
    let installed: () => void = () => undefined;
    const calls = setup(
      {
        mods_updates: () =>
          new Promise<typeof offers>((resolve) => {
            answers.push(resolve);
          }),
        mods_install: () =>
          named('mods_install', calls).length === 1
            ? new Promise<number>((resolve) => {
                installed = () => {
                  resolve(1);
                };
              })
            : 1,
      },
      'installed',
    );
    const check = await screen.findByRole('button', { name: 'Check for updates' });
    check.focus();
    fireEvent.click(check);
    // Checking, it waits but keeps the focus (a disabled button drops it in WebView2).
    expect(check).toHaveTextContent('Checking…');
    expect(check).toHaveAttribute('aria-disabled', 'true');
    expect(check).toHaveFocus();
    fireEvent.click(check);
    expect(named('mods_updates', calls)).toHaveLength(1);
    answers[0]?.(offers);
    expect(await screen.findByText('2 files have updates.')).toBeVisible();

    const all = screen.getByRole('button', { name: 'Update all (2)' });
    all.focus();
    fireEvent.click(all);
    expect(await screen.findByText('Updating 0 of 2…')).toHaveAttribute('role', 'status');
    expect(all).toHaveAttribute('aria-disabled', 'true');
    expect(all).toHaveFocus();
    fireEvent.click(all);
    await waitFor(() => {
      expect(named('mods_install', calls)).toHaveLength(1);
    });
    installed();
    // While the check after it runs, the old offers are not said again.
    expect(await screen.findByText('Everything is updated.')).toHaveAttribute('role', 'status');
    expect(named('mods_install', calls)).toHaveLength(2);
    expect(named('mods_plan', calls)).toHaveLength(2);
    // Nothing is offered until the check answers: the button goes, and the focus goes
    // back to Check.
    expect(screen.queryByRole('button', { name: /Update all/ })).toBeNull();
    await waitFor(() => {
      expect(check).toHaveFocus();
    });
    expect(check).toHaveTextContent('Checking…');
    expect(named('mods_updates', calls)).toHaveLength(2);
    answers[1]?.([]);
    // What "Update all" did, then what the check after it found, in the same line.
    expect(
      await screen.findByText('Everything is updated. Everything the catalog knows is up to date.'),
    ).toHaveAttribute('role', 'status');
  });

  it('keeps the focus on a switch while its file is renamed, and greys out while the game runs', async () => {
    let off = false;
    const calls = setup(
      {
        mods_set_enabled: () => {
          off = true;
          return null;
        },
        // Turned off, the file gets its ".disabled" name.
        mods_content: () =>
          off
            ? {
                ...content,
                items: content.items.map((i, n) =>
                  n === 0 ? { ...i, path: 'mods/sodium.jar.disabled', enabled: false } : i,
                ),
              }
            : content,
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    const sodium = within(mods).getByRole('switch', { name: '⁨Sodium⁩ is on' });
    sodium.focus();
    fireEvent.click(sodium);
    await waitFor(() => {
      expect(named('mods_set_enabled', calls)).toHaveLength(1);
    });
    await waitFor(() => {
      expect(sodium).not.toBeChecked();
    });
    // The same switch, still in its row, still with the focus.
    expect(sodium).toBeInTheDocument();
    expect(sodium).toHaveFocus();
    cleanup();

    // While the game runs nothing here changes, and the controls look it.
    setup({}, 'installed', fabric.id, [{ ...fabric, busy: true }]);
    const busy = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(
      screen.getByText('The installation is busy; changes wait until it is free.'),
    ).toBeVisible();
    expect(within(busy).getByRole('switch', { name: '⁨Sodium⁩ is on' })).toBeDisabled();
    for (const button of within(busy).getAllByRole('button', { name: 'Remove' })) {
      expect(button).toBeDisabled();
    }
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save as a modpack' })).toBeDisabled();
  });

  it('gives the focus to Check for updates once a harmful file it removed is gone', async () => {
    let removed = false;
    let harmful: (paths: string[]) => void = () => undefined;
    setup(
      {
        mods_remove: () => {
          removed = true;
          return null;
        },
        // The warning's list comes back after the folder's.
        mods_harmful: () =>
          removed
            ? new Promise<string[]>((resolve) => {
                harmful = resolve;
              })
            : ['mods/mine.jar.disabled'],
      },
      'installed',
    );
    expect(await screen.findByText('Harmful files found')).toBeVisible();
    const remove = screen.getAllByRole('button', { name: 'Remove' })[0] as HTMLElement;
    remove.focus();
    fireEvent.click(remove);
    await waitFor(() => {
      expect(removed).toBe(true);
    });
    harmful([]);
    await waitFor(() => {
      expect(screen.queryByText('Harmful files found')).toBeNull();
    });
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Check for updates' })).toHaveFocus();
    });
  });

  it('asks in place before removing or turning off, and gives the focus back to what asked', async () => {
    setup(
      {
        mods_needed_by: () => ({ neededBy: ['Iris'], unused: [] }),
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    const remove = within(mods).getAllByRole('button', { name: 'Remove' })[1] as HTMLElement;
    // Each row's buttons are told apart by the row's name.
    expect(remove).toHaveAccessibleDescription('mine.jar');
    remove.focus();
    fireEvent.click(remove);
    const question = await within(mods).findByRole('alertdialog', { name: 'mine.jar' });
    expect(question).toHaveAccessibleDescription(
      'Remove \u2068mine.jar\u2069 from the installation? The file is deleted. ' +
        'Needed by \u2068Iris\u2069; they will not start without it.',
    );
    await waitFor(() => {
      expect(question).toHaveFocus();
    });
    fireEvent.click(within(question).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(within(mods).getAllByRole('button', { name: 'Remove' })[1]).toHaveFocus();
    });

    const sodium = within(mods).getByRole('switch', { name: '\u2068Sodium\u2069 is on' });
    sodium.focus();
    fireEvent.click(sodium);
    const off = await within(mods).findByRole('alertdialog', { name: 'Sodium' });
    fireEvent.keyDown(off, { key: 'Escape' });
    await waitFor(() => {
      expect(sodium).toHaveFocus();
    });
  });

  it('shows a letter for a file without a picture, and says when the folder cannot open', async () => {
    const calls = setup(
      {
        open_instance_folder: () => {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw { code: 'PERMISSION_DENIED', message: 'The folder cannot be opened.' };
        },
      },
      'installed',
    );
    const mods = await screen.findByRole('region', { name: /^Mods \d/ });
    expect(within(mods).getByText('M')).toHaveAttribute('aria-hidden', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Open mods folder' }));
    expect(await screen.findByRole('alert')).toBeVisible();
    expect(named('open_instance_folder', calls)[0]?.args).toMatchObject({
      request: { payload: { instanceId: fabric.id, folder: 'mods' } },
    });
  });

  it('adds to the catalog by name and lets the player choose', async () => {
    const request: ModsImportRequest = {
      id: '00000000-0000-4000-8000-000000000099',
      input: 'sodium',
      state: 'choosing',
      error: null,
      projectId: null,
      candidates: [
        {
          source: 'modrinth',
          id: 'AANobbMI',
          kind: 'mod',
          title: 'Sodium',
          summary: 'Fast',
          author: 'jellysquid',
          downloads: 10,
          projectId: null,
          icon: PICTURE,
          banner: PICTURE,
        },
      ],
      created: 1,
      finished: null,
    };
    const calls = setup({
      mods_import: () => request,
      mods_choose: () => ({ ...request, state: 'importing', candidates: [] }),
    });
    fireEvent.change(await screen.findByLabelText('Search'), { target: { value: 'sodium' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add to the catalog' }));
    const dialog = screen.getByRole('dialog', { name: 'Add to the catalog' });
    expect(within(dialog).getByLabelText('Link or name')).toHaveValue('sodium');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));
    expect(await within(dialog).findByText('Choose a project')).toBeVisible();
    // The catalog's copies of its icon and poster.
    expect(dialog.querySelectorAll(`img[src="${PICTURE}"]`)).toHaveLength(2);
    fireEvent.click(within(dialog).getByRole('button', { name: 'This one' }));
    expect(await within(dialog).findByText('Being added')).toBeVisible();
    expect(named('mods_choose', calls)[0]?.args).toEqual({
      request: request.id,
      source: 'modrinth',
      id: 'AANobbMI',
    });
  });

  it('waits for a request with a spinner, then says the dialog can be closed', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const request: ModsImportRequest = {
        id: '00000000-0000-4000-8000-000000000098',
        input: 'https://modrinth.com/mod/sodium',
        state: 'importing',
        error: null,
        projectId: null,
        candidates: [],
        created: 1,
        finished: null,
      };
      const calls = setup({ mods_import: () => request, mods_import_state: () => request });
      fireEvent.change(await screen.findByLabelText('Search'), {
        target: { value: request.input },
      });
      fireEvent.click(screen.getByRole('button', { name: 'Add to the catalog' }));
      const dialog = screen.getByRole('dialog', { name: 'Add to the catalog' });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));
      expect(await within(dialog).findByText('Being added')).toBeVisible();
      expect(within(dialog).getByText('Adding to the catalog…')).toBeVisible();
      expect(within(dialog).queryByText(/You can close this/)).toBeNull();
      expect(within(dialog).queryByText(/also stays/)).toBeNull();
      // Asked about every second while the dialog waits.
      await vi.advanceTimersByTimeAsync(3000);
      expect(named('mods_import_state', calls).length).toBeGreaterThanOrEqual(2);

      await vi.advanceTimersByTimeAsync(PATIENCE_MS);
      expect(await within(dialog).findByText(/You can close this/)).toBeVisible();
      expect(within(dialog).queryByText('Adding to the catalog…')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('publishes an installation as a pack once its files can go in', async () => {
    const check = (missing: string[]) => ({
      check: {
        linked: 3,
        bundled: 2,
        bytes: 2048,
        missing,
        refused: [],
        tooBig: [],
        published: null,
      },
      pack: null,
      name: 'Modded',
    });
    const calls = setup(
      {
        mods_publish_check: (args) =>
          (args.choice as { worlds: boolean }).worlds ? check([]) : check(['mods/mine.jar']),
        mods_publish: () => ({
          outcome: 'published',
          projectId: PROJECT,
          slug: 'modded',
          versionId: VERSION,
        }),
      },
      'installed',
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Publish to the catalog' }));
    const dialog = await screen.findByRole('dialog', { name: 'Publish as a pack in the catalog' });
    expect(await within(dialog).findByText(/catalog does not have these files/)).toBeVisible();
    expect(within(dialog).getByText('mods/mine.jar')).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Publish' })).toBeDisabled();

    fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Include worlds' }));
    expect(await within(dialog).findByText(/from the catalog 3; copied in 2/)).toBeVisible();
    expect(within(dialog).getByLabelText('Title')).toHaveValue('Modded');
    fireEvent.change(within(dialog).getByLabelText('Short description'), {
      target: { value: 'Fast and pretty' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Publish' }));
    expect(await within(dialog).findByText('The pack is in the catalog.')).toBeVisible();
    expect(named('mods_publish', calls)[0]?.args).toEqual({
      payload: {
        instanceId: fabric.id,
        form: {
          title: 'Modded',
          summary: 'Fast and pretty',
          body: '',
          version: '1.0.0',
          changes: '',
          choice: { worlds: true, options: false },
        },
        icon: false,
        asNew: true,
      },
    });
  });

  it('offers a new version of a pack published before, and hides it', async () => {
    const pack = {
      id: PROJECT,
      slug: 'modded',
      title: 'My pack',
      status: 'ready',
      versions: 2,
      latest: '1.1',
    };
    const calls = setup(
      {
        mods_publish_check: () => ({
          check: {
            linked: 1,
            bundled: 0,
            bytes: 0,
            missing: [],
            refused: [],
            tooBig: [],
            published: PROJECT,
          },
          pack,
          name: 'Modded',
        }),
        mods_pack_hidden: () => 'hidden',
        mods_publish: () => ({ outcome: 'refused', code: 'held', paths: ['mods/sodium.jar'] }),
      },
      'installed',
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Publish to the catalog' }));
    const dialog = await screen.findByRole('dialog', { name: 'Publish as a pack in the catalog' });
    expect(await within(dialog).findByText(/Published as My pack: 2 versions/)).toBeVisible();
    expect(within(dialog).getByLabelText('Title')).toHaveValue('My pack');
    expect(within(dialog).getByLabelText('Version number')).toHaveValue('');
    expect(within(dialog).queryByLabelText('Short description')).toBeNull();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Hide from the catalog' }));
    expect(await within(dialog).findByText('The pack is hidden from the catalog.')).toBeVisible();
    expect(named('mods_pack_hidden', calls)[0]?.args).toEqual({ project: PROJECT, hidden: true });

    fireEvent.change(within(dialog).getByLabelText('Version number'), {
      target: { value: '1.2' },
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Publish the new version' }));
    expect(await within(dialog).findByText(/malware check holds back these files/)).toBeVisible();
    const sent = named('mods_publish', calls)[0]?.args.payload as Record<string, unknown>;
    expect(sent.asNew).toBe(false);
    expect((sent.form as Record<string, unknown>).version).toBe('1.2');
  });

  it('reports a player’s pack to the catalog’s owner', async () => {
    const calls = setup({
      mods_project: () => ({ ...project, sources: [{ source: 'own', url: null }] }),
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    // Marked as a player's pack, not one of a catalog's projects.
    expect(within(article).getByText('Player’s pack')).toBeVisible();
    fireEvent.click(within(article).getByRole('button', { name: 'Report' }));
    const form = within(article).getByRole('form', { name: 'Report' });
    fireEvent.change(within(form).getByLabelText('Reason'), { target: { value: 'stolen' } });
    fireEvent.change(within(form).getByLabelText('What is wrong (optional)'), {
      target: { value: 'Copied from another pack' },
    });
    fireEvent.click(within(form).getByRole('button', { name: 'Send the report' }));
    expect(await within(article).findByText(/owner will look at the pack/)).toBeVisible();
    expect(named('mods_report', calls)[0]?.args).toEqual({
      project: PROJECT,
      reason: 'stolen',
      text: 'Copied from another pack',
    });
  });

  it('offers no report for projects from other sites', async () => {
    setup();
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    expect(within(article).queryByRole('button', { name: 'Report' })).toBeNull();
    expect(within(article).queryByText('Player’s pack')).toBeNull();
  });

  it('offers to sign in when the launcher account is needed', async () => {
    setup(
      {
        mods_imports: () => {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw {
            code: 'AUTH_REAUTH_REQUIRED',
            message: 'Sign in to your launcher account to add to the catalog.',
          };
        },
      },
      'requests',
    );
    expect(
      await screen.findByText('Sign in to your launcher account to add to the catalog.'),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeVisible();
  });

  describe('modpacks', () => {
    const packProject: ModsProject = { ...project, kind: 'modpack', slug: 'pack', title: 'Pack' };
    const packVersion: ModsVersion = {
      ...version,
      number: '1.0',
      name: 'Pack 1.0',
      files: [
        {
          name: 'pack-1.0.mrpack',
          size: 2048,
          sha256: null,
          primary: true,
          url: null,
          held: false,
          checked: false,
        },
      ],
    };
    const ready: ModsPack = {
      state: 'ready',
      error: null,
      game: '1.20.1',
      loader: { kind: 'forge', version: '47.2.0' },
      kept: 2,
      total: 2,
      skipped: 1,
      mods: 2,
      totalBytes: 4096,
      approval: null,
    };
    const answers = (pack: (args: Record<string, unknown>) => ModsPack) => ({
      mods_project: () => packProject,
      mods_versions: () => [packVersion],
      mods_pack: pack,
      mods_create_pack_instance: () => instanceFixture({ displayName: 'Pack 1.0' }),
      mods_apply_pack: () => ({ files: 5, keptYours: 0, leftOut: 2, removed: 0 }),
    });

    it('offers a CurseForge modpack (a zip) for install too', async () => {
      setup(
        {
          ...answers(() => ready),
          mods_versions: () => [
            {
              ...packVersion,
              files: [{ name: 'ATM-1.0.zip', size: 2048, sha256: null, primary: true, url: null }],
            },
          ],
        },
        'modpack',
      );
      fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
      const article = await screen.findByRole('article', { name: 'Pack' });
      fireEvent.click(
        await within(article).findByRole('button', { name: 'Install \u20681.0\u2069' }),
      );
      expect(await screen.findByRole('dialog', { name: 'Install Pack' })).toBeVisible();
    });

    it('installs a modpack as a new installation', async () => {
      const calls = setup(
        answers(() => ready),
        'modpack',
      );
      fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
      const article = await screen.findByRole('article', { name: 'Pack' });
      expect(within(article).getByText(/installs as a new installation/)).toBeVisible();
      // Versions are not filtered by the chosen installation.
      await waitFor(() => {
        expect(named('mods_versions_page', calls).length).toBeGreaterThan(1);
      });
      for (const call of named('mods_versions_page', calls)) {
        expect(call.args.query).toMatchObject({ game: null, loader: null });
      }
      fireEvent.click(
        await within(article).findByRole('button', { name: 'Install \u20681.0\u2069' }),
      );
      const dialog = await screen.findByRole('dialog', { name: 'Install Pack' });
      expect(await within(dialog).findByText(/Forge 47.2.0/)).toBeVisible();
      expect(within(dialog).getByText(/2 mods/)).toBeVisible();
      expect(within(dialog).getByText(/1 file is left out/)).toBeVisible();
      expect(named('mods_pack', calls)[0]?.args).toEqual({ version: VERSION, prepare: true });
      const name = within(dialog).getByLabelText('Installation name');
      expect(name).toHaveValue('Pack 1.0');
      fireEvent.change(name, { target: { value: '   ' } });
      expect(within(dialog).getByRole('button', { name: 'Install' })).toBeDisabled();
      fireEvent.change(name, { target: { value: 'My pack' } });
      fireEvent.click(within(dialog).getByRole('button', { name: 'Install' }));
      expect(
        await within(dialog).findByText(/The modpack is installed as “Pack 1.0”/),
      ).toBeVisible();
      expect(within(dialog).getByText(/2 files were left out/)).toBeVisible();
      const create = named('mods_create_pack_instance', calls)[0]?.args.request as {
        payload: Record<string, unknown>;
      };
      expect(create.payload).toEqual({ version: VERSION, displayName: 'My pack' });
      const apply = named('mods_apply_pack', calls)[0]?.args.request as {
        payload: Record<string, unknown>;
      };
      expect(apply.payload).toEqual({
        instanceId: instanceFixture({}).id,
        version: VERSION,
      });
    });

    it('updates an installation made from a modpack, after a backup', async () => {
      const NEWER = '00000000-0000-4000-8000-000000000011';
      const newer: ModsVersion = {
        ...packVersion,
        id: NEWER,
        number: '1.1',
        files: [
          {
            name: 'pack-1.1.mrpack',
            size: 2048,
            sha256: null,
            primary: true,
            url: null,
            held: false,
            checked: false,
          },
        ],
      };
      const calls = setup(
        {
          ...answers(() => ready),
          mods_versions: () => [newer, packVersion],
          mods_content: () => ({
            ...content,
            items: content.items.map((i, n) => ({ ...i, fromPack: n === 0 })),
            pack: {
              projectId: PROJECT,
              versionId: VERSION,
              title: 'Pack',
              versionNumber: '1.0',
              icon: null,
            },
          }),
          // The pack's own file is updated with the pack, not alone.
          mods_updates: () => [
            {
              path: 'mods/sodium.jar',
              projectId: PROJECT,
              fromVersionId: VERSION,
              toVersionId: NEWER,
              toNumber: '0.6.1',
            },
          ],
          mods_update_pack: () => ({ files: 5, keptYours: 2, leftOut: 0, removed: 1 }),
        },
        'installed',
      );
      expect(
        await screen.findByText('Made from the modpack \u2068Pack\u2069 \u20681.0\u2069'),
      ).toBeVisible();
      expect(screen.getByText('From the modpack')).toBeVisible();
      fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
      fireEvent.click(
        await screen.findByRole('button', { name: 'Update the modpack to \u20681.1\u2069' }),
      );
      expect(screen.queryByText('Update: \u20680.6.1\u2069')).toBeNull();
      // The modpack's offer is said once, and nothing claims the rest is up to date.
      expect(screen.getByText('The modpack has a newer version, \u20681.1\u2069.')).toBeVisible();
      expect(screen.queryByText(/up to date/)).toBeNull();
      expect(named('mods_versions', calls)[0]?.args).toMatchObject({
        project: PROJECT,
        game: fabric.versionId,
      });
      const dialog = await screen.findByRole('dialog', { name: 'Update Pack' });
      expect(await within(dialog).findByText(/backs up the whole installation/)).toBeVisible();
      expect(within(dialog).queryByLabelText('Installation name')).toBeNull();
      fireEvent.click(within(dialog).getByRole('button', { name: 'Update' }));
      expect(await within(dialog).findByText('The modpack is updated to 1.1.')).toBeVisible();
      expect(within(dialog).getByText(/2 files you changed were left/)).toBeVisible();
      const update = named('mods_update_pack', calls)[0]?.args.request as {
        payload: Record<string, unknown>;
      };
      expect(update.payload).toEqual({ instanceId: fabric.id, version: NEWER });
      expect(named('mods_apply_pack', calls)).toHaveLength(0);
      expect(named('mods_create_pack_instance', calls)).toHaveLength(0);
    });

    it('saves an installation as a modpack', async () => {
      const calls = setup(
        {
          mods_export_pack: () => ({
            saved: true,
            fileName: 'Modded.mrpack',
            report: { linked: 3, bundled: 2, bytes: 2048 },
          }),
        },
        'installed',
      );
      fireEvent.click(await screen.findByRole('button', { name: 'Save as a modpack' }));
      const dialog = await screen.findByRole('dialog', { name: 'Save as a Modrinth modpack' });
      fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Include worlds' }));
      fireEvent.click(within(dialog).getByRole('button', { name: 'Choose where to save' }));
      expect(await within(dialog).findByText(/Saved Modded.mrpack/)).toBeVisible();
      expect(within(dialog).getByText(/Links to Modrinth: 3; files inside: 2/)).toBeVisible();
      expect(named('mods_export_pack', calls)[0]?.args).toEqual({
        payload: {
          instanceId: fabric.id,
          choice: { worlds: true, options: false },
          title: 'Save as a Modrinth modpack',
        },
      });
    });

    it('says why the catalog could not prepare a modpack', async () => {
      setup(
        answers(() => ({ ...ready, state: 'failed', error: 'unsupported_host' })),
        'modpack',
      );
      fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
      const article = await screen.findByRole('article', { name: 'Pack' });
      fireEvent.click(
        await within(article).findByRole('button', { name: 'Install \u20681.0\u2069' }),
      );
      const dialog = await screen.findByRole('dialog');
      expect(
        await within(dialog).findByText(/from a site the catalog does not fetch/),
      ).toBeVisible();
      expect(within(dialog).queryByRole('button', { name: 'Install' })).toBeNull();
    });
  });
});

/** A page of `count` projects numbered from `from`. */
function hits(from: number, count: number, total: number): ModsSearchPage {
  const first = page.hits[0];
  if (first === undefined) throw new Error('the fixture changed');
  return {
    total,
    hits: Array.from({ length: count }, (_, i): ModsHit => ({
      ...first,
      id: `p-${String(from + i)}`,
      slug: `p-${String(from + i)}`,
      title: `Project ${String(from + i)}`,
    })),
  };
}

const collection = (patch: Partial<ModsCollection>): ModsCollection => ({
  id: 'popular-week',
  preset: true,
  titles: {},
  notes: {},
  game: null,
  loader: null,
  query: { sort: 'trending', since: null, source: null, collection: null },
  count: 20,
  icons: [PICTURE, PICTURE.replace('aaa', 'aab'), PICTURE.replace('aaa', 'aac')],
  names: ['Alpha', 'Beta'],
  ...patch,
});

describe('Catalog', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    window.localStorage.clear();
  });

  it('lists the categories with icons and counts, and narrows by several at once', async () => {
    const calls = setup({
      mods_categories: () => [
        { id: 'mod.optimization', count: 41 },
        { id: 'mod.utility', count: 63 },
        { id: 'mod.fun', count: 0 },
        // Junk the panel leaves out.
        { id: 'resourcepack.fonts', count: 5 },
        { id: '<script>', count: 1 },
        { id: 'mod.magic', count: -1 },
        { id: 'mod.food', count: Number.NaN },
        { id: 'mod.storage', count: '7' },
        null,
      ],
    });
    const panel = await screen.findByRole('complementary', { name: 'Categories' });
    // Before the results in the focus order, as it is drawn beside or above them.
    expect(
      panel.compareDocumentPosition(screen.getByRole('tabpanel')) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    const rows = within(panel).getAllByRole('button');
    // "All categories" first, then the kind's list in its order.
    expect(rows.map((row) => row.textContent)).toEqual([
      'Categories',
      'All categories1',
      'Performance41',
      'Utility & QoL63',
      'Maps & info',
      'Libraries',
      'Technology',
      'Magic',
      'Adventure & RPG',
      'World generation',
      'Mobs & creatures',
      'Equipment & weapons',
      'Decoration & building',
      'Food & cooking',
      'Storage',
      'Transport',
      'Game mechanics',
      'Server & admin',
      'Social & streaming',
      'Mod add-ons',
      'Minigames & fun0',
    ]);
    expect(within(panel).getByRole('button', { name: /^All categories/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    // Nothing to find there.
    expect(within(panel).getByRole('button', { name: /^Minigames & fun/ })).toBeDisabled();
    expect(named('mods_categories', calls)[0]?.args.query).toMatchObject({
      kind: 'mod',
      game: '1.21.1',
      loader: 'fabric',
      categories: [],
    });

    fireEvent.click(within(panel).getByRole('button', { name: /^Performance/ }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({
      categories: ['mod.optimization'],
      offset: 0,
    });
    fireEvent.click(within(panel).getByRole('button', { name: /^Utility/ }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(3);
    });
    expect(named('mods_search', calls)[2]?.args.query).toMatchObject({
      categories: ['mod.optimization', 'mod.utility'],
    });
    // The counts are for the search without its categories: not asked again.
    expect(named('mods_categories', calls)).toHaveLength(1);
    expect(screen.getByRole('heading', { name: '2 categories' })).toBeVisible();

    const chosen = screen.getByRole('group', { name: 'Chosen filters' });
    fireEvent.click(within(chosen).getByRole('button', { name: 'Remove “Performance”' }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(4);
    });
    expect(named('mods_search', calls)[3]?.args.query).toMatchObject({
      categories: ['mod.utility'],
    });
    expect(screen.getByRole('heading', { name: 'Utility & QoL' })).toHaveFocus();
    fireEvent.click(within(chosen).getByRole('button', { name: 'Reset' }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(5);
    });
    expect(named('mods_search', calls)[4]?.args.query).not.toHaveProperty('categories');
    expect(screen.queryByRole('group', { name: 'Chosen filters' })).toBeNull();
  });

  it('takes eight categories at most', async () => {
    const calls = setup();
    const panel = await screen.findByRole('complementary', { name: 'Categories' });
    const rows = within(panel)
      .getAllByRole('button')
      .filter((row) => row.getAttribute('aria-pressed') !== null)
      .slice(1);
    for (const [index, row] of rows.slice(0, 8).entries()) {
      fireEvent.click(row);
      await waitFor(() => {
        expect(named('mods_search', calls)).toHaveLength(index + 2);
      });
    }
    expect(rows[8]).toBeDisabled();
    expect(rows[7]).toBeEnabled();
    expect(within(panel).getByRole('status')).toHaveTextContent('At most 8 categories at once.');
    const last = named('mods_search', calls).at(-1)?.args.query as { categories: string[] };
    expect(last.categories).toHaveLength(8);
  });

  it('draws the screen when the catalog sends no counts, collections or requests', async () => {
    // The defaults answer null to these.
    const calls = setup();
    expect(await screen.findByText('Sodium')).toBeVisible();
    await waitFor(() => {
      expect(named('mods_collections', calls)).toHaveLength(1);
    });
    expect(screen.queryByRole('region', { name: 'Collections' })).toBeNull();
    expect(screen.getByRole('button', { name: /^Performance$/ })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'My requests' })).toBeVisible();
    expect(named('mods_search', calls)).toHaveLength(1);
  });

  it('counts the requests still on their way beside My requests', async () => {
    const request = (state: ModsImportRequest['state']): ModsImportRequest => ({
      id: state,
      input: 'x',
      state,
      error: null,
      projectId: null,
      candidates: [],
      created: 0,
      finished: null,
    });
    setup({
      mods_imports: () => [request('queued'), request('importing'), request('ready')],
    });
    expect(
      await screen.findByRole('button', { name: 'My requests 2 requests are waiting' }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: /^My requests/ }));
    expect(await screen.findByRole('region', { name: 'My requests' })).toBeVisible();
    expect(screen.getByRole('button', { name: /^My requests/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('shows the catalog’s collections and lists one', async () => {
    const own = collection({
      id: 'fps',
      preset: false,
      titles: { en: 'x'.repeat(64), ru: 'Больше кадров' },
      notes: { en: 'Faster, same picture' },
      game: '1.21.1',
      loader: 'fabric',
      query: { sort: null, since: null, source: null, collection: 'fps' },
      count: 12,
    });
    const calls = setup({
      mods_collections: () => [
        collection({}),
        collection({
          id: 'new',
          query: { sort: 'new', since: 30, source: null, collection: null },
          count: 3,
        }),
        own,
        collection({ id: 'fourth', preset: false, titles: { ru: 'Четвёртая' }, count: 2 }),
      ],
    });
    const strip = await screen.findByRole('region', { name: 'Collections' });
    expect(named('mods_collections', calls)[0]?.args).toEqual({
      kind: 'mod',
      game: '1.21.1',
      loader: 'fabric',
    });
    // Three on a page.
    expect(within(strip).getAllByRole('listitem')).toHaveLength(3);
    const popular = within(strip).getByRole('button', { name: 'Popular this week' });
    expect(popular).toHaveAccessibleDescription('The projects whose downloads grew most this week');
    // Three icons and the rest counted.
    expect(within(popular).getByText('+17')).toBeVisible();
    expect(
      within(strip).getByRole('button', { name: 'New in the catalog' }),
    ).toHaveAccessibleDescription('Added to the catalog in the last 30 days');
    // The owner's title, as text, in English when there is none in the language.
    const mine = within(strip).getByRole('button', { name: 'x'.repeat(64) });
    // Whether it fits is read out too.
    expect(mine).toHaveAccessibleDescription('Faster, same picture Fits your installation');
    expect(within(mine).getByText('Fits your installation')).toBeVisible();
    // The owner's words take their own direction; the launcher's take the interface's.
    expect(within(mine).getByText('Faster, same picture')).toHaveAttribute('dir', 'auto');
    expect(within(popular).getByText(/grew most/)).not.toHaveAttribute('dir');

    const next = within(strip).getByRole('button', { name: 'Next collections' });
    next.focus();
    fireEvent.click(next);
    // A title in no language the launcher speaks still shows.
    expect(within(strip).getByRole('button', { name: 'Четвёртая' })).toHaveAccessibleDescription(
      'Alpha, Beta',
    );
    // Only project names: their own direction.
    expect(within(strip).getByText('Alpha, Beta')).toHaveAttribute('dir', 'auto');
    // At the end the arrow does nothing more, and keeps the focus.
    expect(next).toHaveAttribute('aria-disabled', 'true');
    expect(next).toHaveFocus();
    fireEvent.click(next);
    expect(within(strip).getByRole('button', { name: 'Четвёртая' })).toBeVisible();
    fireEvent.click(within(strip).getByRole('button', { name: 'All collections' }));
    expect(within(strip).getAllByRole('listitem')).toHaveLength(4);

    fireEvent.click(within(strip).getByRole('button', { name: 'Popular this week' }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({
      sort: 'trending',
      offset: 0,
    });
    const heading = screen.getByRole('heading', { name: 'Popular this week' });
    expect(heading).toHaveFocus();
    // Isolated, as an owner's title would need.
    expect(heading.querySelector('bdi')).not.toBeNull();
    // Chosen, the strip gives way to the list.
    expect(screen.queryByRole('region', { name: 'Collections' })).toBeNull();
    expect(screen.getByRole('group', { name: 'Chosen filters' })).toHaveTextContent(
      'Popular this week',
    );
    expect(
      within(screen.getByRole('group', { name: 'Sort by' })).getByRole('button', {
        name: 'Popular this week',
      }),
    ).toHaveAttribute('aria-pressed', 'true');
  });

  it('lists an owner’s collection by its key, in the owner’s order', async () => {
    const calls = setup({
      mods_collections: () => [
        collection({
          id: 'fps',
          preset: false,
          titles: { en: 'More frames' },
          query: { sort: null, since: null, source: null, collection: 'fps' },
          // The strip's count is for its own filters only.
          count: 2,
        }),
      ],
      mods_search: (args) =>
        (args.query as { collection?: string }).collection === 'fps' ? hits(0, 3, 3) : page,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'More frames' }));
    expect(await screen.findByText('Project 2')).toBeVisible();
    // No order of its own: relevance, which the catalog lists in the owner's order.
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({
      collection: 'fps',
      sort: null,
    });
    expect(
      within(screen.getByRole('group', { name: 'Sort by' })).getByRole('button', {
        name: 'Relevance',
      }),
    ).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('3 projects for Minecraft 1.21.1 and Fabric')).toBeVisible();
  });

  it('stops the week’s popular at 20 under any filters, and drops it for another order', async () => {
    const calls = setup({
      mods_collections: () => [
        collection({ count: 5 }),
        collection({
          id: 'new',
          query: { sort: 'new', since: 30, source: null, collection: null },
          count: 3,
        }),
      ],
      mods_search: (args) => hits((args.query as { offset: number }).offset, 20, 300),
    });
    const strip = await screen.findByRole('region', { name: 'Collections' });
    fireEvent.click(within(strip).getByRole('button', { name: 'Popular this week' }));
    expect(await screen.findByText('20 projects for Minecraft 1.21.1 and Fabric')).toBeVisible();
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Loader' })).getByRole('button', {
        name: 'Any loader',
      }),
    );
    // The strip's 5 was for Fabric; the week's list is 20 whatever the filters.
    expect(await screen.findByText('20 projects for Minecraft 1.21.1')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();

    fireEvent.click(
      within(screen.getByRole('group', { name: 'Sort by' })).getByRole('button', { name: 'Name' }),
    );
    expect(await screen.findByText('300 projects for Minecraft 1.21.1')).toBeVisible();
    expect(screen.getByRole('heading', { name: 'All mods' })).toBeVisible();
    expect(screen.queryByRole('group', { name: 'Chosen filters' })).toBeNull();
    expect(named('mods_search', calls).at(-1)?.args.query).toMatchObject({ sort: 'title' });

    // Other collections hold what the catalog finds, not the strip's count.
    const back = await screen.findByRole('region', { name: 'Collections' });
    fireEvent.click(within(back).getByRole('button', { name: 'New in the catalog' }));
    await waitFor(() => {
      expect(named('mods_search', calls).at(-1)?.args.query).toMatchObject({
        sort: 'new',
        since: 30,
      });
    });
    expect(await screen.findByText('300 projects for Minecraft 1.21.1')).toBeVisible();
    // Another order keeps it: its days still make it.
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Sort by' })).getByRole('button', {
        name: 'Downloads',
      }),
    );
    await waitFor(() => {
      expect(named('mods_search', calls).at(-1)?.args.query).toMatchObject({
        sort: 'downloads',
        since: 30,
      });
    });
    expect(screen.getByRole('heading', { name: 'New in the catalog' })).toBeVisible();
  });

  it('adds a project’s newest fitting version from its tile', async () => {
    const forge: ModsVersion = { ...version, id: 'v-forge', loaders: ['forge'] };
    const beta: ModsVersion = { ...version, id: 'v-beta', channel: 'beta' };
    const calls = setup({ mods_versions: () => [forge, beta, version] });
    await screen.findByText('Sodium');
    const add = screen.getByRole('button', { name: 'Add' });
    expect(add).toHaveAccessibleDescription('Sodium');
    fireEvent.click(add);
    expect(await screen.findByRole('dialog', { name: 'Add Sodium to Modded' })).toBeVisible();
    expect(named('mods_versions', calls)[0]?.args).toEqual({
      project: PROJECT,
      game: null,
      loader: null,
    });
    // A release before a beta, and only what the installation runs.
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toMatchObject({ version: VERSION });
  });

  it('opens the project from its tile when no version fits', async () => {
    setup({ mods_versions: () => [{ ...version, loaders: ['forge'] }] });
    await screen.findByText('Sodium');
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    // The tile is an article of the same name: the page's way back says it opened.
    expect(await screen.findByRole('button', { name: 'Back to results' })).toBeVisible();
    const article = await screen.findByRole('article', { name: 'Sodium' });
    expect(
      await within(article).findByRole('button', { name: 'Add to installation' }),
    ).toBeDisabled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('offers no Add without an installation or on modpacks', async () => {
    setup({}, 'mod', null as unknown as string);
    expect(await screen.findByText('Sodium')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Add' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Open' })).toBeVisible();
    expect(screen.queryByRole('switch')).toBeNull();
  });

  it('loads the next page as the list end nears, with the search as sent', async () => {
    const watched = new Set<IntersectionObserverCallback>();
    class Watcher {
      callback: IntersectionObserverCallback;
      constructor(callback: IntersectionObserverCallback) {
        this.callback = callback;
        watched.add(callback);
      }
      observe() {
        // Nothing: the test calls back.
      }
      disconnect() {
        watched.delete(this.callback);
      }
    }
    const near = () => {
      const entries = [{ isIntersecting: true }] as IntersectionObserverEntry[];
      for (const callback of watched) callback(entries, {} as IntersectionObserver);
    };
    vi.stubGlobal('IntersectionObserver', Watcher);
    const calls = setup({
      mods_search: (args) => {
        const { offset } = args.query as { offset: number };
        return hits(offset, offset === 0 ? 20 : 5, 25);
      },
    });
    expect(await screen.findByText('Project 0')).toBeVisible();
    // Typed but not sent: the next page keeps the search the list shows.
    fireEvent.change(screen.getByLabelText('Search'), { target: { value: 'typed' } });
    near();
    expect(await screen.findByText('Project 24')).toBeVisible();
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({ text: '', offset: 20 });
    // All 25 are there: nothing more is watched for.
    await waitFor(() => {
      expect(watched.size).toBe(0);
    });
    near();
    expect(named('mods_search', calls)).toHaveLength(2);
  });

  it('offers to try again when a later page does not come', async () => {
    let fail = true;
    const calls = setup({
      mods_search: (args) => {
        const { offset } = args.query as { offset: number };
        if (offset > 0 && fail) throw new Error('offline');
        return hits(offset, offset === 0 ? 20 : 5, 25);
      },
    });
    // Without IntersectionObserver (jsdom), the list ends with a button.
    fireEvent.click(await screen.findByRole('button', { name: 'Show more' }));
    expect(await screen.findByText('The next projects did not load.')).toBeVisible();
    expect(screen.getByText('Project 19')).toBeVisible();
    fail = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    // The button goes while the page comes: the focus waits on the list's last project.
    expect(screen.getByRole('button', { name: 'Project 19' })).toHaveFocus();
    expect(await screen.findByText('Project 24')).toBeVisible();
    expect(named('mods_search', calls)).toHaveLength(3);
  });

  it('ends the list when a later page brings nothing new', async () => {
    // An observer that answers as soon as it watches, as browsers do.
    class Watcher {
      callback: IntersectionObserverCallback;
      constructor(callback: IntersectionObserverCallback) {
        this.callback = callback;
      }
      observe() {
        queueMicrotask(() => {
          this.callback(
            [{ isIntersecting: true }] as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        });
      }
      disconnect() {
        // Nothing to let go.
      }
    }
    vi.stubGlobal('IntersectionObserver', Watcher);
    // The catalog's order moved: the next page holds projects already listed.
    const calls = setup({
      mods_search: (args) => {
        const { offset } = args.query as { offset: number };
        return offset === 0 ? hits(0, 20, 25) : hits(15, 5, 25);
      },
    });
    expect(await screen.findByText('Project 19')).toBeVisible();
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(named('mods_search', calls)).toHaveLength(2);
  });

  it('notes how far down the list got while it grows', async () => {
    setup({
      mods_search: (args) => {
        const { offset } = args.query as { offset: number };
        return hits(offset, offset === 0 ? 20 : 5, 25);
      },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Show more' }));
    expect(await screen.findByText('Project 24')).toBeVisible();
    // Written now: a run can end with the catalog on screen.
    expect(JSON.parse(window.localStorage.getItem('launcher.mods.place') ?? 'null')).toMatchObject({
      search: JSON.stringify(['mod', '', '1.21.1', 'fabric', 'downloads', [], null]),
      count: 25,
    });
  });

  it('keeps its place when a project opens and the player comes back', async () => {
    const to = vi.fn((_x: number, y: number) => {
      Object.defineProperty(window, 'scrollY', { value: y, configurable: true });
    });
    vi.stubGlobal('scrollTo', to);
    const calls = setup(fresh);
    expect(await screen.findByText('Sodium')).toBeVisible();
    Object.defineProperty(window, 'scrollY', { value: 1500, configurable: true });
    fireEvent.click(screen.getByRole('button', { name: /Sodium/ }));
    expect(await screen.findByText(/Not saved in the catalog yet/)).toBeVisible();
    // Only the page's own way back; the catalog's head and tabs are gone.
    expect(screen.queryByRole('tablist')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Back to results' }));
    expect(await screen.findByRole('tabpanel', { name: 'Mods' })).toBeVisible();
    expect(screen.getByText('Sodium')).toBeVisible();
    expect(named('mods_search', calls)).toHaveLength(1);
    // The page opened at the top, and the list came back where it was.
    expect(to.mock.calls).toEqual([
      [0, 0],
      [0, 1500],
    ]);
    Object.defineProperty(window, 'scrollY', { value: 0, configurable: true });
  });

  it('asks again on return when the new search had not answered', async () => {
    let hold = false;
    const calls = setup({
      ...fresh,
      mods_search: () => (hold ? new Promise(() => undefined) : page),
    });
    expect(await screen.findByText('Sodium')).toBeVisible();
    hold = true;
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Loader' })).getByRole('button', {
        name: 'Any loader',
      }),
    );
    // The old list waits, and the status does not give its count for the new filters.
    const status = within(screen.getByRole('region', { name: 'All mods' })).getByRole('status');
    expect(status).toHaveTextContent('Loading…');
    expect(screen.getByRole('button', { name: /Sodium/ }).closest('ul')).toHaveAttribute(
      'aria-busy',
      'true',
    );
    fireEvent.click(screen.getByRole('button', { name: /Sodium/ }));
    expect(await screen.findByText(/Not saved in the catalog yet/)).toBeVisible();
    hold = false;
    fireEvent.click(screen.getByRole('button', { name: 'Back to results' }));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(3);
    });
    expect(named('mods_search', calls)[2]?.args.query).toMatchObject({ loader: null });
    expect(await screen.findByText('1 project for Minecraft 1.21.1')).toBeVisible();
  });

  it('opens the add dialog only for the last tile pressed', async () => {
    const answers: ((list: ModsVersion[]) => void)[] = [];
    const first = page.hits[0];
    if (first === undefined) throw new Error('the fixture changed');
    const beta: ModsHit = { ...first, id: 'p-beta', slug: 'beta', title: 'Beta' };
    const calls = setup({
      mods_search: () => ({ total: 2, hits: [first, beta] }),
      mods_versions: () =>
        new Promise<ModsVersion[]>((resolve) => {
          answers.push(resolve);
        }),
    });
    expect(await screen.findByText('Beta')).toBeVisible();
    const [sodium, other] = screen.getAllByRole('button', { name: 'Add' });
    if (sodium === undefined || other === undefined) throw new Error('two tiles');
    fireEvent.click(sodium);
    fireEvent.click(other);
    await waitFor(() => {
      expect(answers).toHaveLength(2);
    });
    // Sodium's answer comes late: it opens nothing.
    answers[0]?.([version]);
    answers[1]?.([{ ...version, id: 'v-beta', projectId: 'p-beta' }]);
    expect(await screen.findByRole('dialog', { name: 'Add Beta to Modded' })).toBeVisible();
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toMatchObject({ version: 'v-beta' });
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
  });

  it('drops a quick add when “Add to the catalog” opens meanwhile', async () => {
    const answers: ((list: ModsVersion[]) => void)[] = [];
    setup({
      mods_versions: () =>
        new Promise<ModsVersion[]>((resolve) => {
          answers.push(resolve);
        }),
    });
    expect(await screen.findByText('Sodium')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => {
      expect(answers).toHaveLength(1);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add to the catalog' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add to the catalog' });
    fireEvent.change(within(dialog).getByRole('textbox'), { target: { value: 'my words' } });
    // Nothing fits: without the drop, the project page would open and close the dialog.
    answers[0]?.([]);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(within(dialog).getByRole('textbox')).toHaveValue('my words');
    expect(screen.queryByText(/Not saved in the catalog yet/)).toBeNull();
  });

  it('adds the version made for the installation’s game from any kind’s tile', async () => {
    const first = page.hits[0];
    if (first === undefined) throw new Error('the fixture changed');
    const newer: ModsVersion = { ...version, id: 'v-1214', games: ['1.21.4'], loaders: [] };
    const fitting: ModsVersion = { ...version, id: 'v-1211', games: ['1.21.1'], loaders: [] };
    const calls = setup(
      {
        mods_search: () => ({ total: 1, hits: [{ ...first, kind: 'resourcepack', loaders: [] }] }),
        mods_versions: () => [newer, fitting],
      },
      'resourcepack',
    );
    expect(await screen.findByText('Sodium')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toMatchObject({ version: 'v-1211' });
  });

  it('starts “Add to the catalog” from the open tab’s words only', async () => {
    setup();
    expect(await screen.findByText('Sodium')).toBeVisible();
    fireEvent.change(screen.getByLabelText('Search'), { target: { value: 'sodium' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add to the catalog' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add to the catalog' });
    expect(within(dialog).getByRole('textbox')).toHaveValue('sodium');
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    for (const tab of ['Installed', 'Shaders']) {
      fireEvent.click(screen.getByRole('tab', { name: tab }));
      fireEvent.click(screen.getByRole('button', { name: 'Add to the catalog' }));
      const again = await screen.findByRole('dialog', { name: 'Add to the catalog' });
      expect(within(again).getByRole('textbox')).toHaveValue('');
      fireEvent.keyDown(window, { key: 'Escape' });
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
    }
  });

  it('marks where an earlier run stopped on the same search', async () => {
    // A mark from an earlier run: before this one started.
    window.localStorage.setItem(
      'launcher.mods.place',
      JSON.stringify({
        search: JSON.stringify(['mod', '', '1.21.1', 'fabric', 'downloads', [], null]),
        count: 2,
        at: Date.now() - 86_400_000,
      }),
    );
    setup({ mods_search: () => hits(0, 4, 4) });
    expect(await screen.findByText('Project 3')).toBeVisible();
    const items = screen.getAllByRole('listitem').map((item) => item.textContent);
    const at = items.findIndex((text) => text.startsWith('You stopped here'));
    expect(items[at]).toBe('You stopped here yesterday');
    expect(items[at + 1]).toMatch(/^Project 2/);
  });

  it('keeps the mark when the player comes back before the first search answered', async () => {
    window.localStorage.setItem(
      'launcher.mods.place',
      JSON.stringify({
        search: JSON.stringify(['mod', '', '1.21.1', 'fabric', 'downloads', [], null]),
        count: 2,
        at: Date.now() - 86_400_000,
      }),
    );
    let hold = true;
    setup({
      mods_search: (args) =>
        hold && (args.query as { kind: string }).kind === 'mod'
          ? new Promise(() => undefined)
          : hits(0, 4, 4),
    });
    await screen.findByRole('tabpanel', { name: 'Mods' });
    fireEvent.click(screen.getByRole('tab', { name: 'Shaders' }));
    hold = false;
    fireEvent.click(screen.getByRole('tab', { name: 'Mods' }));
    expect(await screen.findByText('Project 3')).toBeVisible();
    expect(screen.getByText('You stopped here yesterday')).toBeVisible();
  });

  it('forgets a mark that is unsound', async () => {
    window.localStorage.setItem('launcher.mods.place', '{"search":1,"count":"2"}');
    setup({ mods_search: () => hits(0, 4, 4) });
    expect(await screen.findByText('Project 3')).toBeVisible();
    expect(screen.queryByText(/You stopped here/)).toBeNull();
  });

  it('turns off what fits the installation in one search', async () => {
    const calls = setup();
    const fits = await screen.findByRole('switch', { name: 'Only what fits the installation' });
    expect(fits).toBeChecked();
    fireEvent.click(fits);
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({ game: null, loader: null });
    expect(screen.getByRole('switch')).not.toBeChecked();
    fireEvent.click(screen.getByRole('switch'));
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(3);
    });
    expect(named('mods_search', calls)[2]?.args.query).toMatchObject({
      game: '1.21.1',
      loader: 'fabric',
    });
  });

  it('says what to do when nothing is found', async () => {
    const calls = setup({ mods_search: () => ({ total: 0, hits: [] }) });
    expect(await screen.findByText('Nothing found.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Show what does not fit too' }));
    expect(screen.getByRole('heading', { name: 'All mods' })).toHaveFocus();
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Ask the catalog to bring it' }));
    expect(await screen.findByRole('dialog', { name: 'Add to the catalog' })).toBeVisible();
  });

  it('says when the catalog does not answer and tries again', async () => {
    let fail = true;
    const calls = setup({
      mods_search: () => {
        if (fail) throw new Error('offline');
        return page;
      },
    });
    const retry = await screen.findByRole('button', { name: 'Try again' });
    fail = false;
    fireEvent.click(retry);
    // The notice goes while the search runs: the focus waits on the list's heading.
    expect(screen.getByRole('heading', { name: 'All mods' })).toHaveFocus();
    expect(await screen.findByText('Sodium')).toBeVisible();
    expect(named('mods_search', calls)).toHaveLength(2);
  });

  it('drops the old list when a new search fails', async () => {
    let fail = false;
    setup({
      mods_search: () => {
        if (fail) throw new Error('offline');
        return page;
      },
    });
    expect(await screen.findByText('Sodium')).toBeVisible();
    fail = true;
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Loader' })).getByRole('button', {
        name: 'Any loader',
      }),
    );
    expect(await screen.findByRole('button', { name: 'Try again' })).toBeVisible();
    // Its projects answered other filters: they do not stay under the new ones.
    expect(screen.queryByText('Sodium')).toBeNull();
  });

  it('refuses a game version the catalog would not take', async () => {
    const calls = setup();
    await screen.findByText('Sodium');
    fireEvent.change(screen.getByLabelText('Game version'), {
      target: { value: '1.21<script>' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Find' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/letters, digits and dots/);
    expect(screen.getByLabelText('Game version')).toHaveAttribute('aria-invalid', 'true');
    expect(named('mods_search', calls)).toHaveLength(1);
  });

  it('moves between the tabs with the arrow keys', async () => {
    setup();
    const tabs = await screen.findByRole('tablist', { name: 'Sections' });
    const mods = within(tabs).getByRole('tab', { name: 'Mods' });
    expect(mods).toHaveAttribute('aria-selected', 'true');
    expect(
      within(tabs)
        .getAllByRole('tab')
        .map((tab) => tab.tabIndex),
    ).toEqual([0, -1, -1, -1, -1, -1, -1]);
    mods.focus();
    fireEvent.keyDown(mods, { key: 'ArrowRight' });
    const packs = within(tabs).getByRole('tab', { name: 'Resource packs' });
    expect(packs).toHaveFocus();
    expect(packs).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(packs, { key: 'End' });
    expect(within(tabs).getByRole('tab', { name: 'Installed' })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? tabs, { key: 'ArrowRight' });
    expect(mods).toHaveFocus();
    fireEvent.keyDown(mods, { key: 'ArrowLeft' });
    expect(within(tabs).getByRole('tab', { name: 'Installed' })).toHaveFocus();
    fireEvent.keyDown(document.activeElement ?? tabs, { key: 'Home' });
    expect(mods).toHaveFocus();
    expect(screen.getByRole('tabpanel', { name: 'Mods' })).toBeVisible();
  });
});

describe('Project page', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  /** Opens Sodium's page from its tile. */
  async function openSodium() {
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    expect(await screen.findByRole('button', { name: 'Back to results' })).toBeVisible();
    const article = await screen.findByRole('article', { name: 'Sodium' });
    // The focus starts on the project's name.
    await waitFor(() => {
      expect(within(article).getByRole('heading', { level: 2 })).toHaveFocus();
    });
    return article;
  }

  const file = (
    projectId: string,
    versionId: string,
    path: string,
  ): ContentView['items'][number] => ({
    path,
    folder: 'mods',
    name: path.slice('mods/'.length),
    world: null,
    enabled: true,
    size: 1024,
    directory: false,
    projectId,
    versionId,
    title: null,
    versionNumber: null,
    icon: null,
    dependency: false,
    source: null,
    fromPack: false,
  });

  it('shows the kind, categories, authors, facts and the project’s sites', async () => {
    setup({
      ...fresh,
      mods_project: () => ({
        ...project,
        authors: ['jellysquid', 'IMS', 'Kroppeb', 'Rob'].map((name) => ({ name, role: '' })),
        sources: [
          { source: 'modrinth', url: 'https://modrinth.com/mod/sodium' },
          { source: 'external', url: 'https://example.com/sodium' },
        ],
      }),
    });
    const article = await openSodium();
    const card = within(article).getByRole('region', { name: 'Sodium' });
    expect(within(card).getByText('Mod')).toBeVisible();
    expect(within(card).getByText('Performance')).toBeVisible();
    expect(within(card).getByText('⁨jellysquid, IMS, Kroppeb⁩ and 1 more')).toBeVisible();
    const fact = (name: string) => within(card).getByText(name).nextElementSibling;
    expect(fact('Downloads')).toHaveTextContent('1.5K');
    expect(fact('License')).toHaveTextContent('LGPL-3.0');
    expect(fact('Source')).toHaveTextContent(/^Modrinth$/);
    await waitFor(() => {
      expect(fact('Versions')).toHaveTextContent('1, for ⁨1.21.1⁩');
    });
    expect(within(card).getByRole('button', { name: "Open the author's site" })).toHaveTextContent(
      'Author’s site',
    );
    fireEvent.click(within(card).getByRole('button', { name: 'Open on Modrinth' }));
    const dialog = screen.getByRole('dialog', { name: 'Open this link?' });
    expect(within(dialog).getByText('https://modrinth.com/mod/sodium')).toBeVisible();
  });

  it('says the catalog checked the file Add takes', async () => {
    setup({
      ...fresh,
      mods_versions: () => [
        { ...version, saved: 'all', files: version.files.map((f) => ({ ...f, checked: true })) },
      ],
    });
    const article = await openSodium();
    await waitFor(() => {
      expect(within(article).getByText('Source').nextElementSibling).toHaveTextContent(
        'Modrinth, checked by the catalog',
      );
    });
    // Kept: nothing to save.
    expect(within(article).queryByText(/Not saved in the catalog yet/)).toBeNull();
  });

  it('says when the installation has the version Add would take', async () => {
    const older: ModsVersion = {
      ...version,
      id: 'v-old',
      number: '0.5.0',
      published: 1_750_000_000,
    };
    setup({ mods_versions: () => [version, older] });
    const article = await openSodium();
    expect(
      await within(article).findByText('In “\u2068Modded\u2069”: version \u20680.6.0\u2069.'),
    ).toBeVisible();
    expect(within(article).queryByRole('button', { name: /to installation$/ })).toBeNull();
    const row = (await within(article).findByRole('rowheader', { name: /0\.6\.0/ })).closest('tr');
    expect(within(row as HTMLElement).getByText('Installed')).toBeVisible();
  });

  it('updates an older version in place, from the card or a row', async () => {
    const older: ModsVersion = {
      ...version,
      id: 'v-old',
      number: '0.5.0',
      published: 1_750_000_000,
    };
    const calls = setup({
      mods_versions: () => [version, older],
      mods_content: () => ({ ...NOTHING, items: [file(PROJECT, 'v-old', 'mods/sodium-0.5.jar')] }),
    });
    const article = await openSodium();
    fireEvent.click(
      await within(article).findByRole('button', { name: 'Update to \u20680.6.0\u2069' }),
    );
    expect(await screen.findByRole('dialog', { name: 'Add Sodium to Modded' })).toBeVisible();
    await waitFor(() => {
      expect(named('mods_plan', calls)).toHaveLength(1);
    });
    expect(named('mods_plan', calls)[0]?.args.payload).toEqual({
      instanceId: fabric.id,
      version: VERSION,
      world: null,
      replaces: 'mods/sodium-0.5.jar',
    });
    const row = within(article)
      .getByRole('rowheader', { name: /0\.5\.0/ })
      .closest('tr');
    expect(within(row as HTMLElement).getByText('Installed')).toBeVisible();
  });

  it('asks for an installation first, then shows what fits the one chosen', async () => {
    setup(fresh, 'mod', null as unknown as string);
    const article = await openSodium();
    const choose = await within(article).findByRole('button', { name: 'Choose an installation' });
    expect(
      within(article).getByText('Choose an installation at the top to add a version to it.'),
    ).toBeVisible();
    fireEvent.click(choose);
    const select = screen.getByLabelText('Installation');
    expect(select).toHaveFocus();
    // Nothing is "for the installation" yet: no switch, and no row is a misfit.
    expect(within(article).queryByRole('switch')).toBeNull();
    const row = (await within(article).findByRole('rowheader', { name: /0\.6\.0/ })).closest('tr');
    expect(row).toHaveTextContent(
      'Not for this installation: Choose an installation at the top first.',
    );
    fireEvent.change(select, { target: { value: fabric.id } });
    expect(
      await within(article).findByRole('button', { name: 'Add \u20680.6.0\u2069 to installation' }),
    ).toBeEnabled();
    expect(
      within(article).getByRole('switch', { name: 'Only for “\u2068Modded\u2069”' }),
    ).toBeVisible();
  });

  it('lists what a version depends on, and adds or opens those projects', async () => {
    const API = 'p-api';
    const INDIUM = 'p-indium';
    const none = { versionId: null, icon: null, sourcePage: null };
    const needs: ModsVersion = {
      ...version,
      dependencies: [
        { ...none, kind: 'required', projectId: API, title: 'Fabric API', slug: 'fabric-api' },
        {
          ...none,
          kind: 'optional',
          projectId: INDIUM,
          title: 'Indium',
          slug: 'indium',
          icon: 'https://tracker.example/indium.png',
        },
        {
          ...none,
          kind: 'optional',
          projectId: null,
          title: null,
          slug: 'continuity',
          sourcePage: 'https://modrinth.com/project/continuity',
        },
        {
          ...none,
          kind: 'optional',
          projectId: null,
          title: null,
          slug: 'elsewhere',
          sourcePage: 'http://evil.example/x',
        },
        { ...none, kind: 'incompatible', projectId: 'p-of', title: 'OptiFabric', slug: 'of' },
        { ...none, kind: 'embedded', projectId: 'p-emb', title: 'Inside', slug: 'inside' },
        { ...none, kind: 'required', projectId: API, title: 'Fabric API', slug: 'fabric-api' },
      ],
    };
    const calls = setup({
      mods_versions: (args) =>
        args.project === INDIUM ? [{ ...version, id: 'v-indium', projectId: INDIUM }] : [needs],
      mods_project: (args) =>
        args.project === INDIUM
          ? { ...project, id: INDIUM, slug: 'indium', title: 'Indium' }
          : project,
      mods_content: () => ({ ...NOTHING, items: [file(API, 'v-api', 'mods/fabric-api.jar')] }),
    });
    const article = await openSodium();
    const deps = await within(article).findByRole('region', { name: 'Dependencies' });
    const rows = within(deps).getAllByRole('listitem');
    // Each starts with its icon's letter.
    expect(rows.map((row) => row.textContent)).toEqual([
      'FFabric APIRequiredIn the installation',
      'IIndiumOptionalAdd',
      'CcontinuityOptionalTo the catalog',
      'EelsewhereOptional',
      'OOptiFabricDoes not work with it',
    ]);
    // Only the catalog's own pictures are drawn: Indium's icon is a letter.
    expect(document.querySelector('img[src^="https://tracker.example"]')).toBeNull();

    fireEvent.click(
      within(deps).getByRole('button', { name: 'Add \u2068continuity\u2069 to the catalog' }),
    );
    const asked = await screen.findByRole('dialog', { name: 'Add to the catalog' });
    expect(within(asked).getByRole('textbox')).toHaveValue(
      'https://modrinth.com/project/continuity',
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });

    fireEvent.click(within(deps).getByRole('button', { name: 'Add ⁨Indium⁩' }));
    const adding = await screen.findByRole('dialog', { name: 'Add Indium to Modded' });
    expect(named('mods_versions', calls).at(-1)?.args).toEqual({
      project: INDIUM,
      game: null,
      loader: null,
    });
    // Drawn after an answer, outside the test's own steps: Escape once it listens, which
    // is when it has taken the focus.
    await waitFor(() => {
      expect(adding.contains(document.activeElement)).toBe(true);
    });
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });

    fireEvent.click(within(deps).getByRole('button', { name: 'Indium' }));
    expect(await screen.findByRole('article', { name: 'Indium' })).toBeVisible();
    expect(named('mods_project', calls).at(-1)?.args).toEqual({ project: INDIUM });
  });

  it('lists similar projects for the installation and opens one', async () => {
    const first = page.hits[0];
    if (first === undefined) throw new Error('the fixture changed');
    const lithium: ModsHit = {
      ...first,
      id: 'p-lith',
      slug: 'lithium',
      title: 'Lithium',
      summary: 'Logic',
    };
    const calls = setup({
      ...fresh,
      // The project itself is left out if the catalog sends it.
      mods_similar: () => [lithium, first],
      mods_project: (args) =>
        args.project === 'p-lith' ? { ...project, id: 'p-lith', title: 'Lithium' } : project,
    });
    const article = await openSodium();
    const similar = await within(article).findByRole('region', { name: 'Similar' });
    expect(within(similar).getAllByRole('listitem')).toHaveLength(1);
    expect(within(similar).getByText('Logic')).toBeVisible();
    expect(named('mods_similar', calls)[0]?.args).toEqual({
      project: PROJECT,
      game: '1.21.1',
      loader: 'fabric',
    });
    fireEvent.click(within(similar).getByRole('button', { name: 'Open ⁨Lithium⁩' }));
    expect(await screen.findByRole('article', { name: 'Lithium' })).toBeVisible();
    expect(named('mods_project', calls).at(-1)?.args).toEqual({ project: 'p-lith' });
  });

  it('draws only the catalog’s pictures and shows them at full size from the keyboard', async () => {
    const ours = (n: number) => `https://files-mods.lapi.sh/i/${String(n).repeat(64)}.png`;
    setup({
      ...fresh,
      mods_project: () => ({
        ...project,
        gallery: [
          { url: ours(1), thumb: ours(2), title: 'Caves', description: 'A cave' },
          { url: 'https://cdn.example/x.png', thumb: null, title: 'Theirs', description: '' },
          { url: ours(3), thumb: null, title: '', description: '' },
        ],
      }),
    });
    const article = await openSodium();
    const strip = within(article).getByRole('list', { name: 'Pictures' });
    const thumbs = within(strip).getAllByRole('button');
    expect(thumbs.map((b) => b.getAttribute('aria-label'))).toEqual([
      'Picture 1 of 2: Caves',
      'Picture 2 of 2',
    ]);
    expect(document.querySelector('img[src^="https://cdn.example"]')).toBeNull();
    const first = thumbs[0] as HTMLElement;
    first.focus();
    fireEvent.click(first);
    const dialog = await screen.findByRole('dialog', { name: 'Caves' });
    expect(within(dialog).getByRole('img', { name: 'A cave' })).toHaveAttribute('src', ours(1));
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    const next = await screen.findByRole('dialog', { name: 'Picture 2 of 2' });
    expect(within(next).getByRole('img')).toHaveAttribute('src', ours(3));
    fireEvent.keyDown(window, { key: 'ArrowRight' });
    expect(await screen.findByRole('dialog', { name: 'Caves' })).toBeVisible();
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    // Back where it was opened from.
    expect(first).toHaveFocus();
  });

  it('shortens a long description until the player asks for all of it', async () => {
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(900);
    vi.spyOn(Element.prototype, 'clientHeight', 'get').mockReturnValue(240);
    setup(fresh);
    const article = await openSodium();
    const toggle = await within(article).findByRole('button', { name: 'Show in full' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(toggle).toHaveTextContent('Show less');
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    // A link further down takes the focus: the text opens to show it.
    fireEvent.focus(within(article).getByRole('link', { name: 'wiki' }));
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
  });

  it('pages the versions inside their panel as its end comes near', async () => {
    const many = Array.from({ length: 60 }, (_, n) => ({
      ...version,
      id: `v-${String(n)}`,
      number: `0.${String(n)}`,
      published: 1_760_000_000 - n,
      games: ['1.20.1'],
    }));
    class Watcher {
      callback: IntersectionObserverCallback;
      options: IntersectionObserverInit | undefined;
      constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        this.callback = callback;
        this.options = options;
        watchers.push(this);
      }
      observe() {
        queueMicrotask(() => {
          this.callback(
            [{ isIntersecting: true }] as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        });
      }
      disconnect() {
        // Nothing to let go.
      }
    }
    const watchers: Watcher[] = [];
    vi.stubGlobal('IntersectionObserver', Watcher);
    const calls = setup({ ...fresh, mods_versions: () => many });
    const article = await openSodium();
    await waitFor(
      () => {
        expect(within(article).getAllByRole('rowheader')).toHaveLength(60);
      },
      { timeout: 5000 },
    );
    const tables = named('mods_versions_page', calls)
      .map((c) => c.args.query as ModsVersionsQuery)
      .filter((q) => q.limit === 50);
    expect(tables.map((q) => q.offset)).toEqual([0, 50]);
    // Watched inside the panel's own scroll, not the page's.
    expect(watchers[0]?.options?.root).toBe(
      within(article).getByRole('region', { name: 'List of versions' }),
    );
    expect(within(article).getByRole('status', { name: '' })).toBeDefined();
  });

  it('offers the next versions by a button where nothing watches the scroll', async () => {
    const many = Array.from({ length: 55 }, (_, n) => ({
      ...version,
      id: `v-${String(n)}`,
      number: `0.${String(n)}`,
      games: ['1.20.1'],
    }));
    setup({ ...fresh, mods_versions: () => many });
    const article = await openSodium();
    fireEvent.click(await within(article).findByRole('button', { name: 'Load more versions' }));
    await waitFor(() => {
      expect(within(article).getAllByRole('rowheader')).toHaveLength(55);
    });
    expect(within(article).queryByRole('button', { name: 'Load more versions' })).toBeNull();
  });

  it('keeps the answer for the filters chosen last', async () => {
    const later: (() => void)[] = [];
    const two = [
      { ...version, id: 'v-a', number: '0.1', games: ['1.20.1'] },
      { ...version, id: 'v-b', number: '0.2', games: ['1.19.2'] },
    ];
    setup({
      ...fresh,
      mods_versions_page: (args) => {
        const query = args.query as ModsVersionsQuery;
        // The first page for all versions comes after the one for 1.20.1.
        if (query.limit === 50 && query.game === null) {
          return new Promise((resolve) => {
            later.push(() => {
              resolve(pageFrom(two, query));
            });
          });
        }
        return pageFrom(two, query);
      },
    });
    const article = await openSodium();
    fireEvent.change(await within(article).findByLabelText('Game version'), {
      target: { value: '1.20.1' },
    });
    await waitFor(() => {
      expect(
        within(article)
          .getAllByRole('rowheader')
          .map((c) => c.textContent),
      ).toEqual(['0.1']);
    });
    expect(later).toHaveLength(1);
    later[0]?.();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(
      within(article)
        .getAllByRole('rowheader')
        .map((c) => c.textContent),
    ).toEqual(['0.1']);
  });

  it('tries the project and its versions again after they failed', async () => {
    let failProject = true;
    let failVersions = true;
    setup({
      ...fresh,
      mods_project: () => {
        if (failProject) {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw { code: 'NETWORK_UNAVAILABLE', message: 'The catalog is not reachable.' };
        }
        return project;
      },
      mods_versions_page: (args) => {
        if (failVersions) {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw { code: 'NETWORK_UNAVAILABLE', message: 'The catalog is not reachable.' };
        }
        return pageFrom([version], args.query as ModsVersionsQuery);
      },
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    expect(await screen.findByText('The catalog is not reachable.')).toBeVisible();
    failProject = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    const versions = await within(article).findByRole('region', { name: 'Versions' });
    expect(await within(versions).findByText('The versions did not load.')).toBeVisible();
    // The card cannot offer a version either, and says why.
    expect(within(article).getByRole('button', { name: 'Add to installation' })).toBeDisabled();
    failVersions = false;
    fireEvent.click(within(versions).getByRole('button', { name: 'Try again' }));
    expect(await within(article).findByRole('rowheader', { name: /0\.6\.0/ })).toBeVisible();
    expect(
      within(article).getByRole('button', { name: 'Add ⁨0.6.0⁩ to installation' }),
    ).toBeEnabled();
  });

  it('shows what is new in a version and saves it to the catalog', async () => {
    const calls = setup({
      ...fresh,
      mods_version: () => ({
        ...version,
        changelog: [{ k: 'p', c: [{ k: 'text', text: 'Faster chunks' }] }],
      }),
    });
    const article = await openSodium();
    const open = await within(article).findByRole('button', {
      name: 'What’s new in ⁨0.6.0⁩',
    });
    open.focus();
    fireEvent.click(open);
    const dialog = await screen.findByRole('dialog', { name: 'What’s new in ⁨0.6.0⁩' });
    expect(await within(dialog).findByText('Faster chunks')).toBeVisible();
    expect(within(dialog).getByText('sodium.jar')).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save to the catalog' }));
    expect(await within(dialog).findByText('The catalog is saving this version.')).toBeVisible();
    expect(named('mods_fetch_version', calls)[0]?.args).toEqual({ version: VERSION });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(open).toHaveFocus();
  });

  it('starts with a mod’s data pack versions when opened from the data packs', async () => {
    const pack: ModsVersion = { ...version, id: 'v-pack', number: '2.0-dp', loaders: ['datapack'] };
    const calls = setup({ ...fresh, mods_versions: () => [version, pack] }, 'datapack');
    const article = await openSodium();
    expect(
      await within(article).findByRole('button', {
        name: 'Add \u20682.0-dp\u2069 to installation',
      }),
    ).toBeEnabled();
    const loaders = await within(article).findByRole('group', { name: 'Loader' });
    expect(within(loaders).getByRole('button', { name: 'Data pack' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await waitFor(() => {
      expect(
        named('mods_versions_page', calls).some(
          (c) => (c.args.query as ModsVersionsQuery).loader === 'datapack',
        ),
      ).toBe(true);
    });
  });

  it('offers no install of a modpack without an archive', async () => {
    setup(
      {
        ...fresh,
        mods_project: () => ({ ...project, kind: 'modpack', title: 'Pack' }),
        mods_versions: () => [{ ...version, files: [] }],
      },
      'modpack',
    );
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Pack' });
    expect(await within(article).findByRole('button', { name: 'Install' })).toBeDisabled();
    expect(within(article).queryByRole('switch')).toBeNull();
  });
  it('keeps a newer installed beta rather than go back to the newest release', async () => {
    const beta: ModsVersion = {
      ...version,
      id: 'v-beta',
      number: '0.7.0-beta',
      channel: 'beta',
      published: 1_770_000_000,
    };
    setup({
      mods_versions: () => [beta, version],
      mods_content: () => ({ ...NOTHING, items: [file(PROJECT, 'v-beta', 'mods/sodium-b.jar')] }),
    });
    const article = await openSodium();
    // Add takes the newest release; the newer beta stays.
    expect(
      await within(article).findByText('In “\u2068Modded\u2069”: version \u20680.7.0-beta\u2069.'),
    ).toBeVisible();
    expect(within(article).queryByRole('button', { name: /^Update to/ })).toBeNull();
  });

  it('compares the dates of an installed version Add’s list has not got', async () => {
    const elsewhere = (published: number): ModsVersion => ({
      ...version,
      id: 'v-other',
      number: '2.0',
      games: ['1.21.2'],
      published,
    });
    for (const [published, fails, expected] of [
      [1_770_000_000, false, 'In “⁨Modded⁩”: version ⁨2.0⁩.'],
      [1_750_000_000, false, 'Update to ⁨0.6.0⁩'],
      // Unread, what is there stays.
      [1_750_000_000, true, 'In “⁨Modded⁩”: version ⁨2.0⁩.'],
    ] as const) {
      const calls = setup({
        mods_versions: () => [version],
        mods_content: () => ({
          ...NOTHING,
          items: [{ ...file(PROJECT, 'v-other', 'mods/sodium-2.jar'), versionNumber: '2.0' }],
        }),
        mods_version: () => {
          if (fails) {
            // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
            throw { code: 'NETWORK_UNAVAILABLE', message: 'The catalog is not reachable.' };
          }
          return elsewhere(published);
        },
      });
      const article = await openSodium();
      expect(await within(article).findByText(expected)).toBeVisible();
      expect(named('mods_version', calls)[0]?.args).toEqual({ version: 'v-other' });
      cleanup();
    }
  });

  it('lists what the installed version needs, not another loader’s build', async () => {
    const none = { versionId: null, icon: null, sourcePage: null };
    const neo: ModsVersion = {
      ...version,
      id: 'v-neo',
      number: '0.6.0-neoforge',
      loaders: ['neoforge'],
      published: version.published + 60,
      dependencies: [
        { ...none, kind: 'required', projectId: 'p-kff', title: 'Kotlin for Forge', slug: 'kff' },
      ],
    };
    const own: ModsVersion = {
      ...version,
      dependencies: [
        { ...none, kind: 'required', projectId: 'p-api', title: 'Fabric API', slug: 'fabric-api' },
      ],
    };
    // The default installation holds 0.6.0 for Fabric.
    setup({ mods_versions: () => [neo, own] });
    const article = await openSodium();
    expect(
      await within(article).findByText('In “\u2068Modded\u2069”: version \u20680.6.0\u2069.'),
    ).toBeVisible();
    const deps = await within(article).findByRole('region', { name: 'Dependencies' });
    expect(within(deps).getByText('For version ⁨0.6.0⁩')).toBeVisible();
    // Nothing is added now: it is only "Required".
    expect(
      within(deps)
        .getAllByRole('listitem')
        .map((row) => row.textContent),
    ).toEqual(['FFabric APIRequired']);
  });

  it('says a modpack’s versions did not load rather than that it has none', async () => {
    setup(
      {
        ...fresh,
        mods_project: () => ({ ...project, kind: 'modpack', title: 'Pack' }),
        mods_versions_page: (args) => {
          if ((args.query as ModsVersionsQuery).limit === 1) {
            return pageFrom([version], args.query as ModsVersionsQuery);
          }
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw { code: 'NETWORK_UNAVAILABLE', message: 'The catalog is not reachable.' };
        },
      },
      'modpack',
    );
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Pack' });
    const card = within(article).getByRole('region', { name: 'Pack' });
    expect(await within(card).findByText('The versions did not load.')).toBeVisible();
    expect(within(card).getByRole('button', { name: 'Install' })).toBeDisabled();
    expect(within(card).queryByText('No versions yet.')).toBeNull();
  });

  it('watches the end of the versions once the ones for the installation come', async () => {
    const many = Array.from({ length: 60 }, (_, n) => ({
      ...version,
      id: `v-${String(n)}`,
      number: `0.${String(n)}`,
      published: 1_760_000_000 - n,
      games: ['1.20.1'],
    }));
    const watched: Element[] = [];
    class Watcher {
      callback: IntersectionObserverCallback;
      constructor(callback: IntersectionObserverCallback) {
        this.callback = callback;
      }
      observe(target: Element) {
        watched.push(target);
        queueMicrotask(() => {
          if (!target.isConnected) return;
          this.callback(
            [{ isIntersecting: true }] as IntersectionObserverEntry[],
            this as unknown as IntersectionObserver,
          );
        });
      }
      disconnect() {
        // Nothing to let go.
      }
    }
    vi.stubGlobal('IntersectionObserver', Watcher);
    const later: (() => void)[] = [];
    setup({
      ...fresh,
      mods_versions: () => many,
      mods_versions_page: (args) => {
        const query = args.query as ModsVersionsQuery;
        // Add's list comes after the table's first page.
        if (query.limit === 100) {
          return new Promise((resolve) => {
            later.push(() => {
              resolve(pageFrom(many, query));
            });
          });
        }
        return pageFrom(many, query);
      },
    });
    const article = await openSodium();
    await waitFor(() => {
      expect(later).toHaveLength(1);
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    later[0]?.();
    await waitFor(
      () => {
        expect(within(article).getAllByRole('rowheader')).toHaveLength(60);
      },
      { timeout: 5000 },
    );
    expect(watched.some((target) => target.isConnected)).toBe(true);
  });

  it('keeps the description’s toggle after it changed size while open', async () => {
    const observers = new Set<() => void>();
    class Sizes {
      callback: () => void;
      constructor(callback: () => void) {
        this.callback = callback;
      }
      observe() {
        observers.add(this.callback);
      }
      disconnect() {
        observers.delete(this.callback);
      }
    }
    vi.stubGlobal('ResizeObserver', Sizes);
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(900);
    // The shortened box is 240 high; open, it is as high as the text.
    vi.spyOn(Element.prototype, 'clientHeight', 'get').mockImplementation(function (this: Element) {
      return this.className.includes('short') ? 240 : 900;
    });
    setup(fresh);
    const article = await openSodium();
    fireEvent.click(await within(article).findByRole('button', { name: 'Show in full' }));
    // A picture in it loads.
    for (const callback of observers) callback();
    await new Promise((resolve) => setTimeout(resolve, 50));
    fireEvent.click(within(article).getByRole('button', { name: 'Show less' }));
    expect(await within(article).findByRole('button', { name: 'Show in full' })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  it('keeps the focus on the picture buttons while stepping through them', async () => {
    const ours = (n: number) => `https://files-mods.lapi.sh/i/${String(n).repeat(64)}.png`;
    setup({
      ...fresh,
      mods_project: () => ({
        ...project,
        gallery: [1, 2, 3].map((n) => ({ url: ours(n), thumb: null, title: '', description: '' })),
      }),
    });
    const article = await openSodium();
    const strip = within(article).getByRole('list', { name: 'Pictures' });
    fireEvent.click(within(strip).getAllByRole('button')[0] as HTMLElement);
    const dialog = await screen.findByRole('dialog', { name: 'Picture 1 of 3' });
    const next = within(dialog).getByRole('button', { name: 'Next picture' });
    next.focus();
    fireEvent.click(next);
    expect(await screen.findByRole('dialog', { name: 'Picture 2 of 3' })).toBeVisible();
    expect(next).toHaveFocus();
    fireEvent.click(next);
    expect(await screen.findByRole('dialog', { name: 'Picture 3 of 3' })).toBeVisible();
  });

  it('closes only the dialog on top with Escape', async () => {
    // What the installation holds comes while both dialogs are open.
    let held: (view: ContentView) => void = () => undefined;
    setup({
      mods_content: () =>
        new Promise((resolve) => {
          held = resolve;
        }),
      mods_version: () => ({
        ...version,
        changelog: [
          {
            k: 'p',
            c: [{ k: 'a', href: 'https://example.com/notes', c: [{ k: 'text', text: 'notes' }] }],
          },
        ],
      }),
    });
    const article = await openSodium();
    fireEvent.click(
      await within(article).findByRole('button', { name: 'What’s new in \u20680.6.0\u2069' }),
    );
    const details = await screen.findByRole('dialog', { name: 'What’s new in ⁨0.6.0⁩' });
    fireEvent.click(await within(details).findByRole('link', { name: 'notes' }));
    const confirm = await screen.findByRole('dialog', { name: 'Open this link?' });
    await waitFor(() => {
      expect(confirm.contains(document.activeElement)).toBe(true);
    });
    // The page under them draws again: the dialog under the prompt keeps off the focus.
    held(NOTHING);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(confirm.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Open this link?' })).toBeNull();
    });
    expect(screen.getByRole('dialog', { name: 'What’s new in ⁨0.6.0⁩' })).toBeVisible();
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });

  it('lets go of "Only for" when no installation is chosen', async () => {
    setup({ ...fresh, mods_versions: () => [{ ...version, games: ['1.20.1'] }] });
    const article = await openSodium();
    const versions = await within(article).findByRole('region', { name: 'Versions' });
    fireEvent.click(
      await within(versions).findByRole('switch', { name: 'Only for “\u2068Modded\u2069”' }),
    );
    expect(
      await within(versions).findByText('No version here fits “\u2068Modded\u2069”.'),
    ).toBeVisible();
    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: '' } });
    expect(await within(article).findByRole('rowheader', { name: /0\.6\.0/ })).toBeVisible();
    expect(within(article).queryByText(/No version here fits/)).toBeNull();
  });

  it('adds a mod’s data pack next to the mod, and its dependencies as mods', async () => {
    const INDIUM = 'p-indium';
    const pack: ModsVersion = {
      ...version,
      id: 'v-pack',
      number: '2.0-dp',
      loaders: ['datapack'],
      dependencies: [
        {
          kind: 'optional',
          projectId: INDIUM,
          versionId: null,
          title: 'Indium',
          slug: 'indium',
          icon: null,
          sourcePage: null,
        },
      ],
    };
    // The default installation holds the mod's own 0.6.0 jar.
    setup(
      {
        mods_versions: (args) =>
          args.project === INDIUM
            ? [{ ...version, id: 'v-indium', projectId: INDIUM }]
            : [version, pack],
      },
      'datapack',
    );
    const article = await openSodium();
    expect(
      await within(article).findByRole('button', {
        name: 'Add \u20682.0-dp\u2069 to installation',
      }),
    ).toBeEnabled();
    expect(within(article).queryByText(/^In “⁨Modded⁩”/)).toBeNull();
    const deps = await within(article).findByRole('region', { name: 'Dependencies' });
    fireEvent.click(within(deps).getByRole('button', { name: 'Add ⁨Indium⁩' }));
    expect(await screen.findByRole('dialog', { name: 'Add Indium to Modded' })).toBeVisible();
  });

  it('keeps the offer for the installation chosen last', async () => {
    const older = instanceFixture({
      id: '0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f9',
      displayName: 'Older',
      versionId: '1.20.1',
      loader: { kind: 'fabric', version: '0.16.14' },
    });
    const two = [version, { ...version, id: 'v-old', number: '0.5.0', games: ['1.20.1'] }];
    const later: (() => void)[] = [];
    setup(
      {
        ...fresh,
        mods_versions: () => two,
        mods_versions_page: (args) => {
          const query = args.query as ModsVersionsQuery;
          // Add's list for the first installation comes last.
          if (query.limit === 100 && query.game === '1.21.1') {
            return new Promise((resolve) => {
              later.push(() => {
                resolve(pageFrom(two, query));
              });
            });
          }
          return pageFrom(two, query);
        },
      },
      'mod',
      fabric.id,
      [fabric, older],
    );
    const article = await openSodium();
    await waitFor(() => {
      expect(later).toHaveLength(1);
    });
    fireEvent.change(screen.getByLabelText('Installation'), { target: { value: older.id } });
    expect(
      await within(article).findByRole('button', { name: 'Add ⁨0.5.0⁩ to installation' }),
    ).toBeEnabled();
    later[0]?.();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(
      within(article).getByRole('button', { name: 'Add ⁨0.5.0⁩ to installation' }),
    ).toBeEnabled();
  });

  it('tries the list of versions again, and its next page', async () => {
    const many = Array.from({ length: 55 }, (_, n) => ({
      ...version,
      id: `v-${String(n)}`,
      number: `0.${String(n)}`,
      games: ['1.20.1'],
    }));
    let failFirst = true;
    let failNext = true;
    setup({
      ...fresh,
      mods_versions: () => many,
      mods_versions_page: (args) => {
        const query = args.query as ModsVersionsQuery;
        if (query.limit === 50 && (query.offset === 0 ? failFirst : failNext)) {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
          throw { code: 'NETWORK_UNAVAILABLE', message: 'The catalog is not reachable.' };
        }
        return pageFrom(many, query);
      },
    });
    const article = await openSodium();
    const versions = await within(article).findByRole('region', { name: 'Versions' });
    expect(await within(versions).findByText('The versions did not load.')).toBeVisible();
    failFirst = false;
    fireEvent.click(within(versions).getByRole('button', { name: 'Try again' }));
    await waitFor(() => {
      expect(within(versions).getAllByRole('rowheader')).toHaveLength(50);
    });
    fireEvent.click(within(versions).getByRole('button', { name: 'Load more versions' }));
    expect(await within(versions).findByText('The next versions did not load.')).toBeVisible();
    failNext = false;
    fireEvent.click(within(versions).getByRole('button', { name: 'Try again' }));
    await waitFor(() => {
      expect(within(versions).getAllByRole('rowheader')).toHaveLength(55);
    });
  });
  it('moves the focus into a dialog once its controls come', async () => {
    let answer: (plan: ModsPlan) => void = () => undefined;
    setup({
      ...fresh,
      mods_plan: () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    });
    const article = await openSodium();
    const add = await within(article).findByRole('button', {
      name: 'Add \u20680.6.0\u2069 to installation',
    });
    add.focus();
    fireEvent.click(add);
    const dialog = await screen.findByRole('dialog', { name: 'Add Sodium to Modded' });
    // Nothing to press while the plan is worked out: the dialog holds the focus.
    await waitFor(() => {
      expect(dialog).toHaveFocus();
    });
    answer(plan);
    await waitFor(() => {
      expect(within(dialog).getByRole('button', { name: 'Install' })).toBeEnabled();
    });
    await waitFor(() => {
      expect(dialog.contains(document.activeElement) && document.activeElement !== dialog).toBe(
        true,
      );
    });
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(add).toHaveFocus();
  });
});
