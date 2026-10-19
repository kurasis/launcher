import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type {
  ContentView,
  DocNode,
  ModsImportRequest,
  ModsPlan,
  ModsProject,
  ModsSearchPage,
  ModsVersion,
} from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
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
  files: [{ name: 'sodium.jar', size: 1024, sha256: null, primary: true, url: null }],
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
    },
  ],
  worlds: [],
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
      case 'mods_content':
        return content;
      case 'mods_updates':
      case 'mods_identify':
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
    expect(named('mods_versions', calls)[0]?.args).toMatchObject({
      project: PROJECT,
      game: '1.21.1',
      loader: 'fabric',
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
    fireEvent.click(within(mods).getByRole('button', { name: 'Remove the file' }));
    await waitFor(() => {
      expect(named('mods_remove', calls)).toHaveLength(1);
    });
    const removed = named('mods_remove', calls)[0]?.args.request as {
      payload: Record<string, unknown>;
    };
    expect(removed.payload).toEqual({ instanceId: fabric.id, path: 'mods/mine.jar.disabled' });
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
    fireEvent.click(within(dialog).getByRole('button', { name: 'This one' }));
    expect(await within(dialog).findByText('Being added')).toBeVisible();
    expect(named('mods_choose', calls)[0]?.args).toEqual({
      request: request.id,
      source: 'modrinth',
      id: 'AANobbMI',
    });
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
});
