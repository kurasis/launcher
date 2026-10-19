import { describe, expect, it } from 'vitest';
import type { AppSnapshot, RunSummary } from '../lib/contracts';
import { snapshotFixture } from '../test/fixtures';
import { menuViews, rememberedView, rowViews, startViewOf } from './navigation';

function withSettings(settings: Partial<AppSnapshot['settings']>, more: Partial<AppSnapshot> = {}) {
  return snapshotFixture({ ...more, settings: { ...snapshotFixture().settings, ...settings } });
}

describe('navigation', () => {
  it('keeps every screen in one place', () => {
    expect(rowViews).toEqual(['home', 'account', 'settings']);
    expect(menuViews).toEqual(['backups', 'screenshots', 'diagnostics', 'about']);
  });

  it('remembers only start screens', () => {
    expect(rememberedView('home')).toBe('home');
    expect(rememberedView('account')).toBe('account');
    for (const view of ['settings', ...menuViews] as const) {
      expect(rememberedView(view)).toBeNull();
    }
  });

  it('picks the start screen', () => {
    expect(startViewOf(withSettings({ startView: 'account' }))).toBe('account');
    expect(startViewOf(withSettings({ startView: 'home' }))).toBe('home');
    expect(startViewOf(withSettings({ startView: 'last', lastView: 'account' }))).toBe('account');
    // Screens this version does not have yet.
    expect(startViewOf(withSettings({ startView: 'feed' }))).toBe('home');
    expect(startViewOf(withSettings({ startView: 'mods' }))).toBe('home');
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
