import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { JavaCheck } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstanceJavaForm } from './InstanceJavaForm';

const installed = instanceFixture({
  health: 'ready',
  java: { provider: 'mojang', major: 21, release: '21.0.7' },
});

function backend(check: Partial<JavaCheck> = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'check_java') {
      return {
        path: '/opt/jdk-17/bin/java',
        javaVersion: '17.0.9',
        major: 17,
        vendor: 'Eclipse Adoptium',
        arch: 'amd64',
        requiredMajor: 21,
        issues: ['older'],
        ...check,
      } satisfies JavaCheck;
    }
    if (cmd === 'set_instance_java') return installed;
    if (cmd === 'repair_instance') return { operationId: 'op-1' };
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return calls;
}

const saved = (calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === 'set_instance_java').at(-1)?.args;

describe('InstanceJavaForm', () => {
  it('switches the private runtime source and offers the repair that downloads it', async () => {
    const calls = backend();
    const { rerender } = render(<InstanceJavaForm instance={installed} />);
    expect(screen.getByText('Now: Mojang · Java 21 (21.0.7)')).toBeVisible();
    fireEvent.click(screen.getByRole('radio', { name: /from Eclipse Temurin/ }));
    fireEvent.change(screen.getByLabelText('JVM options'), {
      target: { value: '-XX:+UseG1GC' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save Java settings' }));
    await waitFor(() => {
      expect(saved(calls)).toMatchObject({
        request: {
          payload: {
            java: { kind: 'managed', provider: 'temurin' },
            jvmArguments: '-XX:+UseG1GC',
            acceptIssues: false,
          },
        },
      });
    });
    rerender(<InstanceJavaForm instance={{ ...installed, javaProvider: 'temurin' }} />);
    expect(screen.getByText(/Java from Eclipse Temurin \(Adoptium\) is downloaded/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Repair now' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'repair_instance')).toBe(true);
    });
  });

  it('warns about an unsuitable Java and saves it only after confirmation', async () => {
    const calls = backend();
    render(<InstanceJavaForm instance={installed} />);
    fireEvent.click(screen.getByRole('radio', { name: /My own Java/ }));
    fireEvent.change(screen.getByLabelText('Path to the Java program'), {
      target: { value: '/opt/jdk-17/bin/java' },
    });
    const save = screen.getByRole('button', { name: 'Save Java settings' });
    save.focus();
    fireEvent.click(save);
    // Save closes while the warnings ask (WebView2 drops the focus of a closed button):
    // the focus goes to their Cancel, and back to Save when they go.
    save.blur();
    expect(await screen.findByText(/older than the Java 21 this game needs/)).toBeVisible();
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(save).toHaveFocus();
    fireEvent.click(save);
    expect(await screen.findByText(/older than the Java 21 this game needs/)).toBeVisible();
    expect(
      screen.getByText('Java 17.0.9 (Eclipse Adoptium, amd64). This game was built for Java 21.'),
    ).toBeVisible();
    expect(calls.some((c) => c.cmd === 'set_instance_java')).toBe(false);
    const useAnyway = screen.getByRole('button', { name: 'Use anyway' });
    useAnyway.focus();
    fireEvent.click(useAnyway);
    await waitFor(() => {
      expect(saved(calls)).toMatchObject({
        request: {
          payload: {
            java: { kind: 'custom', path: '/opt/jdk-17/bin/java' },
            acceptIssues: true,
          },
        },
      });
    });
    expect(await screen.findByText('Saved.')).toBeVisible();
    await waitFor(() => {
      expect(save).toHaveFocus();
    });
  });

  it('saves a matching Java without asking', async () => {
    const calls = backend({ major: 21, javaVersion: '21.0.5', issues: [] });
    render(<InstanceJavaForm instance={installed} />);
    fireEvent.click(screen.getByRole('radio', { name: /My own Java/ }));
    fireEvent.change(screen.getByLabelText('Path to the Java program'), {
      target: { value: '/opt/jdk-17/bin/java' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save Java settings' }));
    await waitFor(() => {
      expect(saved(calls)).toMatchObject({ request: { payload: { acceptIssues: false } } });
    });
    expect(screen.queryByRole('button', { name: 'Use anyway' })).toBeNull();
  });

  it('shows the chosen Java and its accepted warning', () => {
    backend();
    render(
      <InstanceJavaForm
        instance={{
          ...installed,
          customJava: {
            path: '/opt/jdk-25/bin/java',
            javaVersion: '25.0.1',
            major: 25,
            accepted: ['newer'],
          },
        }}
      />,
    );
    expect(screen.getByText('Now: Your own Java 25.0.1')).toBeVisible();
    expect(screen.getByRole('radio', { name: /My own Java/ })).toBeChecked();
    expect(screen.getByLabelText('Path to the Java program')).toHaveValue('/opt/jdk-25/bin/java');
    expect(screen.getByText(/newer than the Java 21/)).toBeVisible();
    // Another path is not the Java whose warning was accepted.
    fireEvent.change(screen.getByLabelText('Path to the Java program'), {
      target: { value: '/opt/jdk-21/bin/java' },
    });
    expect(screen.queryByText(/newer than the Java 21/)).toBeNull();
  });

  it('asks to install first before choosing an own Java', () => {
    backend();
    render(<InstanceJavaForm instance={instanceFixture()} />);
    expect(screen.getByRole('radio', { name: /My own Java/ })).toBeDisabled();
    expect(screen.getByText(/Install this version first/)).toBeVisible();
  });
});
