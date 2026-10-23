import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AccountSummary } from '../lib/contracts';
import { AccountView } from '../features/accounts/AccountView';
import { HEADS_RETRY_MS } from '../features/accounts/useMicrosoftHeads';
import { accountFixture, snapshotFixture } from '../test/fixtures';
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
  afterEach(() => {
    vi.restoreAllMocks();
  });

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

  it('asks again for a missing head when the window comes back a minute later', async () => {
    let answer: unknown = { alex: null };
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return cmd === 'skins_microsoft_heads' ? answer : null;
    });
    let now = 1_000_000;
    vi.spyOn(Date, 'now').mockImplementation(() => now);
    const { container } = render(
      <RailUser account={accountFixture(microsoft)} activeKind={null} />,
    );
    const asked = () => calls.filter((c) => c === 'skins_microsoft_heads').length;
    await waitFor(() => {
      expect(asked()).toBe(1);
    });
    // Too soon: Mojang is not asked again on every focus.
    now += HEADS_RETRY_MS - 1;
    act(() => {
      fireEvent.focus(window);
    });
    expect(asked()).toBe(1);
    answer = { alex: HEAD };
    now += 1;
    act(() => {
      fireEvent.focus(window);
    });
    await waitFor(() => {
      expect(container.querySelector('img')).toHaveAttribute('src', HEAD);
    });
    expect(asked()).toBe(2);
    // Every head is there now: no more questions.
    now += HEADS_RETRY_MS;
    act(() => {
      fireEvent.focus(window);
    });
    expect(asked()).toBe(2);
  });

  it('shares one question with the Accounts screen', async () => {
    const calls = backend({ alex: HEAD, steve: HEAD });
    const account = accountFixture(microsoft);
    const base = snapshotFixture();
    render(
      <>
        <RailUser account={account} activeKind={null} />
        <AccountView snapshot={{ ...base, account }} />
      </>,
    );
    await waitFor(() => {
      expect(document.querySelectorAll(`img[src="${HEAD}"]`)).toHaveLength(3);
    });
    expect(calls.filter((c) => c === 'skins_microsoft_heads')).toHaveLength(1);
  });
});
