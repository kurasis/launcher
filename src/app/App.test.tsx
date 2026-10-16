import { render, screen } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AppInfo } from '../lib/contracts';
import { App } from './App';

// Test-only IPC fixture. Production code always talks to the real Rust core.
const fixture: AppInfo = {
  productName: 'Kurasis Launcher',
  appVersion: '0.1.0',
  tauriVersion: '2.12.4',
  os: 'linux',
  arch: 'x86_64',
  buildProfile: 'debug',
  microsoftClientId: 'not_configured',
};

describe('App', () => {
  it('renders core facts returned by get_app_info', async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === 'get_app_info') return fixture;
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);

    expect(screen.getByRole('heading', { level: 1, name: 'Kurasis Launcher' })).toBeVisible();
    expect(screen.getByRole('status')).toHaveTextContent('Contacting the launcher core');
    expect(await screen.findByText('linux / x86_64')).toBeVisible();
    expect(screen.getByText('0.1.0')).toBeVisible();
    expect(screen.getByText('Debug')).toBeVisible();
    expect(calls).toEqual(['get_app_info']);
  });

  it('reports a missing Microsoft client ID without offering sign-in', async () => {
    mockIPC(() => fixture);

    render(<App />);

    const setup = await screen.findByRole('region', { name: 'Microsoft sign-in setup' });
    expect(setup).toHaveTextContent('Not configured');
    expect(setup).toHaveTextContent('docs/AUTH-SETUP.md');
    expect(screen.queryByRole('button', { name: /sign in/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /play/i })).not.toBeInTheDocument();
  });

  it('never presents a configured client ID as verified', async () => {
    mockIPC(() => ({ ...fixture, microsoftClientId: 'configured_unverified' }));

    render(<App />);

    const setup = await screen.findByRole('region', { name: 'Microsoft sign-in setup' });
    expect(setup).toHaveTextContent('Configured, not yet verified');
  });

  it('shows an accessible error and recovers on retry', async () => {
    let failures = 1;
    mockIPC(() => {
      if (failures > 0) {
        failures -= 1;
        throw new Error('core unavailable');
      }
      return fixture;
    });

    render(<App />);

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('core unavailable');

    screen.getByRole('button', { name: 'Try again' }).click();
    expect(await screen.findByText('linux / x86_64')).toBeVisible();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('always shows the independent-project notice', () => {
    mockIPC(() => fixture);
    render(<App />);
    expect(screen.getByRole('contentinfo')).toHaveTextContent(
      'not affiliated with or endorsed by Mojang Studios or Microsoft',
    );
  });
});
