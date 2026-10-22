import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CatalogView, VersionView } from '../../lib/contracts';
import { VersionPicker } from './VersionPicker';

const RELEASED = '2024-08-08T12:00:00+00:00';

function version(
  id: string,
  versionType: VersionView['versionType'],
  extra: Partial<VersionView> = {},
): VersionView {
  return {
    id,
    versionType,
    releaseTime: RELEASED,
    verified: false,
    availability: 'available',
    ...extra,
  };
}

function catalog(versions: VersionView[]): CatalogView {
  return {
    versions,
    fetchedAt: '2026-10-17T00:00:00Z',
    stale: false,
    error: null,
    latestRelease: '1.21.1',
    latestSnapshot: '26.4-snapshot-4',
  };
}

const everything = catalog([
  version('26.4-snapshot-4', 'snapshot'),
  version('1.21.1', 'release', { verified: true }),
  version('1.12.2', 'release'),
  version('b1.7.3', 'old_beta'),
  version('rd-132211', 'old_alpha', { releaseTime: null }),
]);

const tabNames = () => screen.getAllByRole('tab').map((tab) => tab.textContent);

afterEach(() => {
  document.documentElement.dir = '';
});

describe('VersionPicker', () => {
  it('offers a tab only for the version types that have versions', () => {
    render(
      <VersionPicker
        catalog={catalog([
          version('26.4-snapshot-4', 'snapshot'),
          version('1.21.1', 'release'),
          version('1.12.2', 'release'),
        ])}
        value="1.21.1"
        onChange={() => undefined}
      />,
    );
    expect(screen.getByRole('tablist', { name: 'Version type' })).toBeVisible();
    expect(tabNames()).toEqual(['Releases2', 'Snapshots1']);
    expect(screen.queryByRole('tab', { name: /Beta/ })).toBeNull();
    expect(screen.queryByRole('tab', { name: /Alpha/ })).toBeNull();
  });

  it('shows no tabs when one version type remains', () => {
    render(
      <VersionPicker
        catalog={everything}
        value="1.21.1"
        allowed={new Set(['1.21.1', '1.12.2'])}
        onChange={() => undefined}
      />,
    );
    expect(screen.queryByRole('tablist')).toBeNull();
    expect(screen.queryByRole('tabpanel')).toBeNull();
    expect(screen.getByRole('list', { name: 'Releases' })).toHaveTextContent('1.12.2');
  });

  it('connects each tab to the panel it shows', () => {
    render(<VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />);
    const releases = screen.getByRole('tab', { name: /Releases/ });
    const panel = screen.getByRole('tabpanel', { name: /Releases/ });
    expect(releases).toHaveAttribute('aria-selected', 'true');
    expect(releases).toHaveAttribute('aria-controls', panel.id);
    expect(within(panel).getByRole('list', { name: 'Releases' })).toBeVisible();
  });

  it('moves between the tabs with the arrow keys, Home and End', () => {
    render(<VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />);
    const tab = (name: RegExp) => screen.getByRole('tab', { name });
    // One tab stop: the chosen tab.
    expect(tab(/Releases/)).toHaveAttribute('tabindex', '0');
    expect(tab(/Snapshots/)).toHaveAttribute('tabindex', '-1');

    fireEvent.keyDown(tab(/Releases/), { key: 'ArrowRight' });
    expect(tab(/Snapshots/)).toHaveAttribute('aria-selected', 'true');
    expect(tab(/Snapshots/)).toHaveFocus();
    expect(tab(/Snapshots/)).toHaveAttribute('tabindex', '0');
    expect(tab(/Releases/)).toHaveAttribute('tabindex', '-1');
    expect(screen.getByRole('list', { name: 'Snapshots' })).toBeVisible();

    fireEvent.keyDown(tab(/Snapshots/), { key: 'End' });
    expect(tab(/Alpha/)).toHaveAttribute('aria-selected', 'true');
    expect(tab(/Alpha/)).toHaveFocus();
    // Past the last tab comes the first.
    fireEvent.keyDown(tab(/Alpha/), { key: 'ArrowRight' });
    expect(tab(/Releases/)).toHaveFocus();
    fireEvent.keyDown(tab(/Releases/), { key: 'ArrowLeft' });
    expect(tab(/Alpha/)).toHaveFocus();
    fireEvent.keyDown(tab(/Alpha/), { key: 'Home' });
    expect(tab(/Releases/)).toHaveFocus();
    expect(tab(/Releases/)).toHaveAttribute('aria-selected', 'true');
  });

  it('turns the arrow keys round right to left', () => {
    document.documentElement.dir = 'rtl';
    render(<VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />);
    fireEvent.keyDown(screen.getByRole('tab', { name: /Releases/ }), { key: 'ArrowLeft' });
    expect(screen.getByRole('tab', { name: /Snapshots/ })).toHaveFocus();
    expect(screen.getByRole('tab', { name: /Snapshots/ })).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(screen.getByRole('tab', { name: /Snapshots/ }), { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: /Releases/ })).toHaveFocus();
  });

  it('follows the selection to its version type', () => {
    const { rerender } = render(
      <VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />,
    );
    expect(screen.getByRole('list', { name: 'Releases' })).toBeVisible();
    rerender(<VersionPicker catalog={everything} value="b1.7.3" onChange={() => undefined} />);
    expect(screen.getByRole('tab', { name: /Beta/ })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('button', { name: /b1\.7\.3/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('keeps a tab the player chose while the selection stays', () => {
    const onChange = vi.fn();
    render(<VersionPicker catalog={everything} value="1.21.1" onChange={onChange} />);
    fireEvent.click(screen.getByRole('tab', { name: /Snapshots/ }));
    expect(screen.getByRole('list', { name: 'Snapshots' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: /^26\.4-snapshot-4/ }));
    expect(onChange).toHaveBeenCalledWith(everything.versions[0]);
  });

  it('leaves a tab that a loader empties', () => {
    const { rerender } = render(
      <VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />,
    );
    fireEvent.click(screen.getByRole('tab', { name: /Snapshots/ }));
    rerender(
      <VersionPicker
        catalog={everything}
        value="1.21.1"
        allowed={new Set(['1.21.1', 'b1.7.3'])}
        onChange={() => undefined}
      />,
    );
    expect(tabNames()).toEqual(['Releases1', 'Beta1']);
    expect(screen.getByRole('tab', { name: /Releases/ })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('list', { name: 'Releases' })).toHaveTextContent('1.21.1');
  });

  it('shows tags in sentence case and dates in the interface language', () => {
    render(<VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />);
    const row = screen.getByRole('button', { name: /^1\.21\.1/ });
    expect(within(row).getByText('Latest')).toBeVisible();
    expect(within(row).getByText('Verified')).toBeVisible();
    const date = new Date(RELEASED).toLocaleDateString('en', { dateStyle: 'medium' });
    expect(within(row).getByText(date)).toHaveAttribute('dateTime', RELEASED);
    expect(row).not.toHaveTextContent('2024-08-08');
  });

  it('leaves out a missing or broken release date', () => {
    render(
      <VersionPicker
        catalog={catalog([
          version('1.21.1', 'release', { releaseTime: 'not a date' }),
          version('1.12.2', 'release', { releaseTime: null }),
        ])}
        value="1.21.1"
        onChange={() => undefined}
      />,
    );
    expect(screen.getByRole('button', { name: /^1\.21\.1/ })).not.toHaveTextContent('Invalid Date');
    expect(screen.getByRole('button', { name: /^1\.12\.2/ }).querySelector('time')).toBeNull();
  });

  it('takes junk and very long text in the search without breaking', () => {
    const { container } = render(
      <VersionPicker catalog={everything} value="1.21.1" onChange={() => undefined} />,
    );
    const search = screen.getByRole('searchbox', { name: 'Find a version' });
    expect(search).toHaveAttribute('maxLength', '32');
    // Markup, pattern characters, a right-to-left override and far too much text.
    const junk = `<img src=x onerror=alert(1)>.*[(‮${'x'.repeat(10_000)}`;
    fireEvent.change(search, { target: { value: junk } });
    expect((search as HTMLInputElement).value).toBe(junk.slice(0, 32));
    expect(screen.getByText('No version matches.')).toBeVisible();
    expect(container.querySelector('img')).toBeNull();
    // Characters that mean something in a pattern are plain text here.
    fireEvent.change(search, { target: { value: '1.2' } });
    expect(screen.getByRole('list', { name: 'Releases' })).toHaveTextContent('1.21.1');
    fireEvent.change(search, { target: { value: '.*' } });
    expect(screen.getByText('No version matches.')).toBeVisible();
    fireEvent.change(search, { target: { value: '  1.12  ' } });
    expect(screen.getByRole('list', { name: 'Releases' })).toHaveTextContent('1.12.2');
    expect(screen.getByRole('list', { name: 'Releases' })).not.toHaveTextContent('1.21.1');
  });

  it('says what limits the list before the version type hint', () => {
    render(
      <VersionPicker
        catalog={everything}
        value="1.21.1"
        note="Only Minecraft versions that Fabric supports are listed."
        onChange={() => undefined}
      />,
    );
    expect(
      screen.getByText('Only Minecraft versions that Fabric supports are listed.'),
    ).toBeVisible();
    expect(screen.getByText('Stable versions.')).toBeVisible();
  });
});
