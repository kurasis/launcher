import { fireEvent, render, screen } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CrashDiagnosis, CrashKind, RunSummary } from '../../lib/contracts';
import { loadLanguage, setLanguage } from '../../lib/strings';
import { instanceFixture } from '../../test/fixtures';
import { crashText } from '../diagnostics/crashText';
import { CrashHelp } from './CrashHelp';

const diagnosis: CrashDiagnosis = {
  kind: 'out_of_memory',
  details: { area: 'Java heap space', memory: '2048', suggested: '4096', exitCode: '-1' },
  evidence: ['java.lang.OutOfMemoryError: Java heap space'],
  suspects: ['Alex’s Mobs (alexsmobs)'],
  report: 'crash-2026-10-18_12.21.54-client.txt',
  actions: ['memory_settings', 'open_crash_reports', 'open_mods'],
  notes: [
    {
      kind: 'mod_for_other_loader',
      details: { file: 'sodium-fabric-0.5.13.jar', loader: 'Fabric' },
    },
  ],
};

const run: RunSummary & { diagnosis: CrashDiagnosis } = {
  runId: 'run-1',
  kind: 'game',
  instanceId: 'i-1',
  state: 'failed',
  startedAt: '2026-10-18T12:00:00Z',
  endedAt: '2026-10-18T12:01:00Z',
  exitCode: -1,
  durationMs: 60000,
  drainTimedOut: false,
  outputTail: [],
  outputLinesDropped: 0,
  loggingDegraded: false,
  error: null,
  signal: null,
  diagnosis,
};

const kinds: CrashKind[] = [
  'jvm_option',
  'heap_unavailable',
  'java_too_old',
  'java_too_new',
  'missing_dependency',
  'wrong_dependency_version',
  'wrong_minecraft_version',
  'loader_too_old',
  'incompatible_mods',
  'duplicate_mods',
  'mod_for_other_loader',
  'corrupt_mod_file',
  'corrupt_config',
  'mixin_failed',
  'mod_failed_to_load',
  'optifine_conflict',
  'out_of_memory',
  'system_memory',
  'killed',
  'opengl_unsupported',
  'graphics_driver',
  'overlay',
  'native_crash',
  'missing_native_library',
  'broken_installation',
  'disk_full',
  'file_access',
  'world_load_failed',
  'ticking_entity',
  'ticking_block_entity',
  'server_hang',
  'debug_crash',
  'game_crash',
  'terminated',
  'unknown',
];

const full: Record<string, string> = {
  option: '-XX:+NoSuchFlag',
  memory: '2048',
  suggested: '4096',
  required: '21',
  found: '17',
  mod: 'Create',
  modId: 'create',
  dependency: 'Fabric API',
  dependencyId: 'fabric-api',
  count: '3',
  files: 'a.jar, b.jar',
  file: 'a.jar',
  loader: 'Fabric',
  place: 'mods',
  error: '65542',
  module: 'nvoglv64.dll',
  vendor: 'nvidia',
  program: 'RTSSHooks64',
  signal: 'SIGSEGV',
  library: 'liblwjgl.so',
  description: 'Ticking entity',
  exception: 'java.lang.NullPointerException',
  entity: 'minecraft:zombie',
  block: 'create:press',
  position: '1, 2, 3',
  seconds: '60',
  exitCode: '1',
};

describe('CrashHelp', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('explains the cause, offers its actions and shows the evidence', () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return null;
    });
    const onEditSettings = vi.fn();
    render(
      <CrashHelp
        instance={instanceFixture({ id: 'i-1' })}
        run={run}
        onEditSettings={onEditSettings}
        onOpenBackups={() => undefined}
        busy={false}
      />,
    );
    const region = screen.getByRole('region', { name: 'Why the game stopped' });
    expect(region).toHaveTextContent('The game ran out of memory');
    expect(region).toHaveTextContent('The game used all 2048 MiB it was given.');
    expect(region).toHaveTextContent('4096 MiB is a good start');
    expect(region).toHaveTextContent('Mods the crash report points at: Alex’s Mobs (alexsmobs)');
    expect(region).toHaveTextContent(
      "sodium-fabric-0.5.13.jar is a Fabric mod, so this installation's loader skips it.",
    );
    expect(region).toHaveTextContent(
      'Exit code -1 · Crash report: crash-2026-10-18_12.21.54-client.txt',
    );
    expect(region).toHaveTextContent('java.lang.OutOfMemoryError: Java heap space');
    fireEvent.click(screen.getByRole('button', { name: 'Change memory' }));
    expect(onEditSettings).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Open crash reports' }));
    expect(calls.find((c) => c.cmd === 'open_instance_folder')?.args).toMatchObject({
      request: { payload: { instanceId: 'i-1', folder: 'crash-reports' } },
    });
  });

  it('has a sentence for every cause in every language, with or without details', async () => {
    for (const tag of ['en', 'ru', 'ar']) {
      setLanguage(tag === 'en' ? 'en' : await loadLanguage(tag));
      for (const kind of kinds) {
        // Without details nothing breaks; with them, every value is used.
        for (const [details, unused] of [
          [{}, /crash\.kinds|\{/],
          [full, /\bnone\b|crash\.kinds|\{/],
        ] as const) {
          const text = crashText({ kind, details });
          for (const part of [text.title, text.cause, text.fix]) {
            expect(part, `${tag} ${kind}`).not.toMatch(unused);
            expect(part.length).toBeGreaterThan(5);
          }
        }
      }
    }
  });
});
