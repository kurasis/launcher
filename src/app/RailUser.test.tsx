import { render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AccountSummary } from '../lib/contracts';
import { accountFixture } from '../test/fixtures';
import { RailUser } from './RailUser';

const HEAD = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAI';

function backend(heads: unknown) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === 'skins_microsoft_heads') return heads;
    return null;
  });
  return calls;
}

const microsoft: Partial<AccountSummary> = {
  accounts: [
    { profileId: 'steve', profileName: 'Steve_Test', state: 'verified_access', error: null },
    { profileId: 'alex', profileName: 'Alex_Test', state: 'verified_access', error: null },
  ],
  defaultProfile: { kind: 'microsoft', id: 'alex' },
};

describe('the player at the foot of the rail', () => {
  it("shows the default Microsoft account's own head", async () => {
    const calls = backend({ steve: `${HEAD}AA`, alex: HEAD });
    const { container } = render(
      <RailUser account={accountFixture(microsoft)} activeKind={null} />,
    );
    expect(screen.getByText('Alex_Test')).toBeVisible();
    await waitFor(() => {
      expect(container.querySelector('img')).toHaveAttribute('src', HEAD);
    });
    expect(calls.filter((c) => c === 'skins_microsoft_heads')).toHaveLength(1);
    expect(calls).not.toContain('skins_mine');
  });

  it('draws the default face for a player without a head', async () => {
    const calls = backend({ steve: HEAD });
    const { container } = render(
      <RailUser account={accountFixture(microsoft)} activeKind={null} />,
    );
    await waitFor(() => {
      expect(calls).toContain('skins_microsoft_heads');
    });
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('svg[viewBox="0 0 8 8"]')).not.toBeNull();
  });

  it('asks Mojang for nothing when a demo profile plays by default', () => {
    const calls = backend({});
    render(<RailUser account={accountFixture()} activeKind={null} />);
    expect(screen.getByText('Player')).toBeVisible();
    expect(calls).not.toContain('skins_microsoft_heads');
  });
});
