import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppSnapshot, FeedItem, ServerAd } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { AD_EVERY, adOrder, withAds } from './list';
import { ServerAdTile } from './ServerAdTile';

function adFixture(overrides: Partial<ServerAd> = {}): ServerAd {
  return {
    id: '7',
    name: 'Northern Land',
    address: 'play.example.net',
    text: 'Survival with clans.\nWeekend events.',
    version: '1.21.4',
    versions: ['1.21.11', '1.21.4', '1.20.1'],
    icon: null,
    up: true,
    online: 12,
    max: 100,
    endsAt: Math.floor(Date.now() / 1000) + 3600,
    weight: 10,
    erid: null,
    ...overrides,
  };
}

type Calls = { cmd: string; args: Record<string, unknown> }[];

function listen(answers: Record<string, unknown> = {}): Calls {
  const calls: Calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return answers[cmd] ?? null;
  });
  return calls;
}

function payload(calls: Calls, cmd: string): Record<string, unknown> | undefined {
  const request = calls.find((c) => c.cmd === cmd)?.args.request as
    { payload: Record<string, unknown> } | undefined;
  return request?.payload;
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe('ServerAdTile', () => {
  it('shows the ad as text, never markup', () => {
    listen();
    render(
      <ServerAdTile
        ad={adFixture({ text: '<img src=x onerror=alert(1)>' })}
        snapshot={snapshotFixture()}
      />,
    );
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeTruthy();
    expect(document.querySelector('img[src="x"]')).toBeNull();
    expect(screen.getByText('Ad')).toBeTruthy();
    expect(screen.getByText('12 of 100 online')).toBeTruthy();
    expect(screen.getByTitle('12 of 100 online').textContent).toContain('12');
    expect(screen.getByText('play.example.net')).toBeTruthy();
  });

  it('joins the server with a ready vanilla installation', async () => {
    const ready = instanceFixture({ id: 'ready-1', versionId: '1.21.4', health: 'ready' });
    const calls = listen();
    render(<ServerAdTile ad={adFixture()} snapshot={snapshotFixture({ instances: [ready] })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Play, 1.21.4 · installed' }));
    await waitFor(() => {
      expect(payload(calls, 'launch_game')).toEqual({
        instanceId: 'ready-1',
        server: 'play.example.net',
      });
    });
    expect(calls.some((c) => c.cmd === 'create_instance')).toBe(false);
  });

  it('installs the version first, then joins', async () => {
    const created = instanceFixture({ id: 'new-1', versionId: '1.21.4', health: 'not_installed' });
    const calls = listen({
      create_instance: created,
      install_instance: { operationId: 'op-1' },
    });
    const base = snapshotFixture({ instances: [] });
    const { rerender } = render(<ServerAdTile ad={adFixture()} snapshot={base} />);
    fireEvent.click(screen.getByRole('button', { name: 'Play, 1.21.4 · downloads first' }));
    await waitFor(() => {
      expect(payload(calls, 'install_instance')).toEqual({ instanceId: 'new-1' });
    });
    expect(payload(calls, 'create_instance')).toEqual({
      displayName: 'Minecraft 1.21.4',
      versionId: '1.21.4',
      loader: null,
    });
    const installing: AppSnapshot = {
      ...base,
      instances: [created],
      activeOperation: { kind: 'install', instanceId: 'new-1', operationId: 'op-1' },
    };
    rerender(<ServerAdTile ad={adFixture()} snapshot={installing} />);
    expect(screen.getByRole('button', { name: /Installing 1.21.4/ })).toBeTruthy();
    expect(calls.some((c) => c.cmd === 'launch_game')).toBe(false);
    rerender(
      <ServerAdTile
        ad={adFixture()}
        snapshot={{ ...base, instances: [{ ...created, health: 'ready' }] }}
      />,
    );
    await waitFor(() => {
      expect(payload(calls, 'launch_game')).toEqual({
        instanceId: 'new-1',
        server: 'play.example.net',
      });
    });
  });

  it('says so when the install ends without a ready installation', async () => {
    const created = instanceFixture({ id: 'new-2', versionId: '1.21.4', health: 'not_installed' });
    listen({ create_instance: created, install_instance: { operationId: 'op-2' } });
    const base = snapshotFixture({ instances: [] });
    const { rerender } = render(<ServerAdTile ad={adFixture()} snapshot={base} />);
    fireEvent.click(screen.getByRole('button', { name: /Play/ }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Installing/ })).toBeTruthy();
    });
    await act(async () => {
      rerender(
        <ServerAdTile
          ad={adFixture()}
          snapshot={{
            ...base,
            instances: [created],
            activeOperation: { kind: 'install', instanceId: 'new-2', operationId: 'op-2' },
          }}
        />,
      );
      await Promise.resolve();
    });
    rerender(
      <ServerAdTile
        ad={adFixture()}
        snapshot={{ ...base, instances: [{ ...created, health: 'repair_required' }] }}
      />,
    );
    expect(await screen.findByRole('alert')).toBeTruthy();
  });

  it('picks another version the server lets in and remembers it', async () => {
    const calls = listen({
      create_instance: instanceFixture({ id: 'n', versionId: '1.20.1', health: 'not_installed' }),
    });
    render(<ServerAdTile ad={adFixture()} snapshot={snapshotFixture({ instances: [] })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Ad options: Northern Land' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Version: 1.21.4…' }));
    expect(screen.getByRole('menu', { name: 'Choose a version for Northern Land' })).toBeTruthy();
    expect(screen.getByText('The server lets in 1.20.1 to 1.21.11')).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole('menuitemradio', { name: /1\.21\.4/ }));
    fireEvent.click(screen.getByRole('menuitemradio', { name: /1\.20\.1/ }));
    expect(screen.queryByRole('menuitemradio')).toBeNull();
    expect(screen.getByRole('button', { name: 'Play, 1.20.1 · downloads first' })).toBeTruthy();
    expect(window.localStorage.getItem('serverAd.version.7')).toBe('1.20.1');
    fireEvent.click(screen.getByRole('button', { name: /Play/ }));
    await waitFor(() => {
      expect(payload(calls, 'create_instance')?.versionId).toBe('1.20.1');
    });
  });

  it('offers no version button for a single version, and forgets one the ad dropped', () => {
    listen();
    window.localStorage.setItem('serverAd.version.7', '1.8.9');
    render(
      <ServerAdTile
        ad={adFixture({ versions: ['1.21.4'] })}
        snapshot={snapshotFixture({ instances: [] })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Ad options: Northern Land' }));
    expect(screen.queryByRole('menuitem', { name: /Version/ })).toBeNull();
    expect(
      screen.getByRole('menuitem', { name: 'Copy the address play.example.net' }),
    ).toBeTruthy();
    expect(screen.getByRole('menuitem', { name: 'Report' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Play, 1.21.4 · downloads first' })).toBeTruthy();
  });

  it('shows a server that does not answer, and the icon as the picture', () => {
    listen();
    const icon = 'data:image/png;base64,iVBORw0KGgo=';
    render(
      <ServerAdTile
        ad={adFixture({ up: false, online: null, max: null, icon })}
        snapshot={snapshotFixture()}
      />,
    );
    expect(screen.getByText('Offline')).toBeTruthy();
    expect(screen.getByText('The server does not answer right now')).toBeTruthy();
    expect(document.querySelectorAll(`img[src="${icon}"]`)).toHaveLength(2);
  });

  it('does not use an installation with a mod loader', () => {
    listen();
    const forge = instanceFixture({
      versionId: '1.21.4',
      health: 'ready',
      loader: { kind: 'forge', version: '54.0.0' },
    });
    render(<ServerAdTile ad={adFixture()} snapshot={snapshotFixture({ instances: [forge] })} />);
    expect(screen.getByRole('button', { name: 'Play, 1.21.4 · downloads first' })).toBeTruthy();
  });
});

describe('server ads in the list', () => {
  const items: FeedItem[] = Array.from({ length: 40 }, (_, i) => ({
    type: 'stream',
    stream: {
      login: `s${String(i)}`,
      name: 'S',
      title: 'T',
      viewers: 1,
      started: '2026-10-19T10:00:00Z',
      image: 'tw/s.jpg',
    },
  }));

  it('puts the first ad second and then one every few tiles', () => {
    const entries = withAds(items, [adFixture({ id: '1' }), adFixture({ id: '2' })]);
    const places = entries.flatMap((entry, index) => (entry.type === 'ad' ? [index] : []));
    expect(places[0]).toBe(1);
    expect(places[1]).toBe(AD_EVERY + 1);
    expect(entries.filter((e) => e.type === 'item')).toHaveLength(40);
    expect(withAds(items, [])).toHaveLength(40);
    expect(withAds(items.slice(0, 1), [adFixture()])).toHaveLength(1);
  });

  it('orders every ad once, the same way for the same seed', () => {
    const ads = [adFixture({ id: '1', weight: 1 }), adFixture({ id: '2', weight: 100 })];
    const order = adOrder(ads, 0.5).map((ad) => ad.id);
    expect(order.sort()).toEqual(['1', '2']);
    expect(adOrder(ads, 0.25)).toEqual(adOrder(ads, 0.25));
    let heavyFirst = 0;
    for (let i = 0; i < 100; i += 1) if (adOrder(ads, i / 100)[0]?.id === '2') heavyFirst += 1;
    expect(heavyFirst).toBeGreaterThan(80);
  });
});
