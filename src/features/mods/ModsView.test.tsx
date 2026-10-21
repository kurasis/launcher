import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type {
  ContentView,
  DocNode,
  ModsImportRequest,
  ModsPack,
  ModsPlan,
  ModsProject,
  ModsSearchPage,
  ModsVersion,
} from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { PATIENCE_MS } from './AddToCatalog';
import { cannotAdd } from './helpers';
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
  files: [{ name: 'sodium.jar', size: 1024, sha256: null, primary: true, url: null, held: false }],
  dependencies: [],
  goneAt: null,
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
      fromPack: false,
    },
  ],
  worlds: [],
  pack: null,
};

type Answers = Record<string, (args: Record<string, unknown>) => unknown>;

function setup(answers: Answers = {}, start: ModsSection = 'mod', instanceId = fabric.id) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(payload);
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
        snapshot={snapshotFixture({ instances: [fabric] })}
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

  it('leaves out empty headings and paragraphs and keeps heading levels apart', () => {
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
    expect(screen.getByRole('heading', { name: 'Big', level: 4 })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Graphics', level: 5 })).toBeVisible();
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
    // Other kinds only warn about the game version, and need no loader.
    expect(
      cannotAdd('resourcepack', { ...version, games: ['1.8.9'], loaders: [] }, vanilla),
    ).toBeNull();
  });
});

describe('ModsView', () => {
  it('searches with the installation’s game version and loader', async () => {
    const calls = setup();
    expect(await screen.findByText('Sodium')).toBeVisible();
    expect(screen.getByText('by jellysquid')).toBeVisible();
    const query = named('mods_search', calls)[0]?.args.query as Record<string, unknown>;
    expect(query).toMatchObject({ kind: 'mod', game: '1.21.1', loader: 'fabric', offset: 0 });

    fireEvent.change(screen.getByLabelText('Loader'), { target: { value: '' } });
    await waitFor(() => {
      expect(named('mods_search', calls)).toHaveLength(2);
    });
    expect(named('mods_search', calls)[1]?.args.query).toMatchObject({ loader: null });
  });

  it('opens a project and adds a version with what it needs', async () => {
    const calls = setup();
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    expect(await within(article).findByText(/Not saved in the catalog yet/)).toBeVisible();
    // Every version comes; the page filters them.
    expect(named('mods_versions', calls)[0]?.args).toMatchObject({
      project: PROJECT,
      game: null,
      loader: null,
    });

    fireEvent.click(await within(article).findByRole('button', { name: 'Add to installation' }));
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

  it('shows on hover why a version cannot be added', async () => {
    setup({ mods_versions: () => [{ ...version, loaders: ['forge'] }] });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    const add = await within(article).findByRole('button', { name: 'Add to installation' });
    expect(add).toBeDisabled();
    const reason = 'This version is for Forge, and “Modded” runs Fabric.';
    expect(add).toHaveAttribute('title', reason);
    // A disabled button gets no hover everywhere: its wrapper says it too.
    expect(add.parentElement).toHaveAttribute('title', reason);
  });

  it('adds a map as a new world', async () => {
    const calls = setup({
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
    fireEvent.click(await within(article).findByRole('button', { name: 'Add to installation' }));
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

  it('filters and orders versions, starting with what fits the installation', async () => {
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
    setup({
      mods_versions: () => [
        v(1, ['1.21.1'], ['fabric'], { saved: 'all' }),
        v(2, ['1.20.1'], ['forge'], { channel: 'beta' }),
        v(3, ['1.20.1', '1.20'], ['fabric', 'quilt'], {}),
      ],
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    const names = () =>
      within(article)
        .queryAllByText(/^Sodium 0\.\d$/)
        .map((e) => e.textContent);
    await waitFor(() => {
      expect(names()).toEqual(['Sodium 0.1']);
    });
    expect(within(article).getByText('In the catalog')).toBeVisible();
    expect(within(article).getByText('1 version of 3')).toBeVisible();
    const game = within(article).getByLabelText('Game version');
    expect(
      within(game)
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['Any', '1.21.1', '1.20.1', '1.20']);
    fireEvent.change(game, { target: { value: '' } });
    expect(names()).toEqual(['Sodium 0.1', 'Sodium 0.3']);
    fireEvent.change(within(article).getByLabelText('Loader'), { target: { value: '' } });
    fireEvent.change(within(article).getByLabelText('Order'), { target: { value: 'oldest' } });
    expect(names()).toEqual(['Sodium 0.3', 'Sodium 0.2', 'Sodium 0.1']);
    fireEvent.change(within(article).getByLabelText('Type'), { target: { value: 'alpha' } });
    expect(within(article).getByText('No version matches the filters.')).toBeVisible();
    fireEvent.click(within(article).getByRole('button', { name: 'Show all' }));
    expect(names()).toHaveLength(3);
  });

  it('does not install while something is missing or in the way', async () => {
    setup({
      mods_plan: () => ({
        ...plan,
        missing: [{ projectId: null, title: 'Fabric API' }],
        conflicts: [{ path: 'mods/sodium.jar', reason: 'name_taken', title: null }],
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    fireEvent.click(await within(article).findByRole('button', { name: 'Add to installation' }));
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findByText('Fabric API')).toBeVisible();
    expect(within(dialog).getByText(/mods\/sodium.jar is already in the folder/)).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Install' })).toBeDisabled();
  });

  it('says when a metered connection asks first', async () => {
    setup({
      mods_plan: () => ({
        ...plan,
        approval: { bytes: 1024, files: 1, reason: 'large', monthBytes: 0, monthlyLimitMib: 0 },
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: /Sodium/ }));
    const article = await screen.findByRole('article', { name: 'Sodium' });
    fireEvent.click(await within(article).findByRole('button', { name: 'Add to installation' }));
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
    const mods = await screen.findByRole('region', { name: 'Mods' });
    expect(within(mods).getByText('Sodium')).toBeVisible();
    expect(within(mods).getByText('Added by hand')).toBeVisible();
    expect(within(mods).getByText('Off')).toBeVisible();

    fireEvent.click(within(mods).getByRole('checkbox', { name: 'Sodium is on' }));
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
    const mods = await screen.findByRole('region', { name: 'Mods' });
    fireEvent.click(within(mods).getByRole('checkbox', { name: 'Sodium is on' }));
    expect(
      await within(mods).findByText('Needed by Iris; they will not start without it.'),
    ).toBeVisible();
    expect(named('mods_set_enabled', calls)).toHaveLength(0);
    fireEvent.click(within(mods).getByRole('button', { name: 'Cancel' }));
    expect(within(mods).queryByText(/Needed by Iris/)).toBeNull();
    fireEvent.click(within(mods).getByRole('checkbox', { name: 'Sodium is on' }));
    fireEvent.click(await within(mods).findByRole('button', { name: 'Turn off anyway' }));
    await waitFor(() => {
      expect(named('mods_set_enabled', calls)).toHaveLength(1);
    });

    // Removing: what only it needed goes too, unless the player says not.
    fireEvent.click(within(mods).getAllByRole('button', { name: 'Remove' })[0] as HTMLElement);
    const also = await within(mods).findByRole('checkbox', {
      name: 'Also remove what only it needed: Library',
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
    expect(await screen.findByText('Update: 0.6.1')).toBeVisible();
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
      fireEvent.click(await within(article).findByRole('button', { name: 'Install' }));
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
        expect(named('mods_versions', calls)[0]?.args).toMatchObject({ game: null, loader: null });
      });
      fireEvent.click(await within(article).findByRole('button', { name: 'Install' }));
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
      expect(await screen.findByText('Made from the modpack Pack 1.0')).toBeVisible();
      expect(screen.getByText('From the modpack')).toBeVisible();
      fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
      fireEvent.click(await screen.findByRole('button', { name: 'Update the modpack to 1.1' }));
      expect(screen.queryByText('Update: 0.6.1')).toBeNull();
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
      fireEvent.click(await within(article).findByRole('button', { name: 'Install' }));
      const dialog = await screen.findByRole('dialog');
      expect(
        await within(dialog).findByText(/from a site the catalog does not fetch/),
      ).toBeVisible();
      expect(within(dialog).queryByRole('button', { name: 'Install' })).toBeNull();
    });
  });
});
