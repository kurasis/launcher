import { describe, expect, it } from 'vitest';
import type { AppSnapshot, RunSummary } from '../lib/contracts';
import { snapshotFixture } from '../test/fixtures';
import { menuViews, rememberedView, rowViews, startViewOf, viewShown } from './navigation';

function withSettings(settings: Partial<AppSnapshot['settings']>, more: Partial<AppSnapshot> = {}) {
  return snapshotFixture({ ...more, settings: { ...snapshotFixture().settings, ...settings } });
}

describe('navigation', () => {
  it('keeps every screen in one place', () => {
    expect(rowViews).toEqual(['feed', 'home', 'mods', 'worlds', 'account', 'settings']);
    expect(menuViews).toEqual(['backups', 'screenshots', 'diagnostics', 'reviews', 'about']);
  });

  it('remembers only start screens', () => {
    expect(rememberedView('feed')).toBe('feed');
    expect(rememberedView('home')).toBe('home');
    expect(rememberedView('mods')).toBe('mods');
    // Rooms need the service: never a start screen.
    expect(rememberedView('worlds')).toBeNull();
    expect(rememberedView('account')).toBe('account');
    for (const view of ['settings', ...menuViews] as const) {
      expect(rememberedView(view)).toBeNull();
    }
  });

  it('picks the start screen', () => {
    expect(startViewOf(withSettings({ startView: 'account' }))).toBe('account');
    expect(startViewOf(withSettings({ startView: 'home' }))).toBe('home');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'account' }))).toBe('account');
    expect(startViewOf(withSettings({ startView: 'feed' }))).toBe('feed');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'feed' }))).toBe('feed');
    // The feed switched off.
    const off = { ...snapshotFixture().settings.feed, enabled: false };
    expect(startViewOf(withSettings({ startView: 'feed', feed: off }))).toBe('home');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'feed', feed: off }))).toBe(
      'home',
    );
    expect(startViewOf(withSettings({ startView: 'mods' }))).toBe('mods');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'mods' }))).toBe('mods');
    // A screen this version does not have yet.
    expect(startViewOf(withSettings({ startView: 'skins' }))).toBe('home');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'skins' }))).toBe('home');
    // Values an older or damaged core could send.
    for (const junk of ['settings', 'about', '', 'ACCOUNT', undefined, null, 7]) {
      const snapshot = withSettings({ startView: junk as never, lastView: junk as never });
      expect(startViewOf(snapshot)).toBe('home');
      expect(
        startViewOf({ ...snapshot, settings: { ...snapshot.settings, startView: 'last' } }),
      ).toBe('home');
    }
  });

  it('shows the feed only while it is on', () => {
    const settings = snapshotFixture().settings;
    expect(viewShown('feed', settings)).toBe(true);
    expect(viewShown('feed', { ...settings, feed: { ...settings.feed, enabled: false } })).toBe(
      false,
    );
    expect(viewShown('home', { ...settings, feed: { ...settings.feed, enabled: false } })).toBe(
      true,
    );
  });

  it('opens Installations while a game or an install runs', () => {
    const running = { runId: 'run-1' } as RunSummary;
    expect(startViewOf(withSettings({ startView: 'account' }, { currentRun: running }))).toBe(
      'home',
    );
    expect(
      startViewOf(
        withSettings(
          { startView: 'last', lastView: 'account' },
          { activeOperation: { kind: 'repair', instanceId: null, operationId: 'op-1' } },
        ),
      ),
    ).toBe('home');
  });
});
