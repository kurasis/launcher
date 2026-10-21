import { describe, expect, it } from 'vitest';

import { MOD_LOADER_KINDS, gameLabel, loaderVersionLabel } from './loaderLabel';

describe('loader labels', () => {
  it('names loader versions the way people do', () => {
    expect(loaderVersionLabel({ kind: 'forge', version: '1.21.1-52.1.0' }, '1.21.1')).toBe(
      '52.1.0',
    );
    expect(loaderVersionLabel({ kind: 'fabric', version: '0.16.14' }, '1.21.1')).toBe('0.16.14');
    expect(loaderVersionLabel({ kind: 'optifine', version: '1.21.1_HD_U_J1' }, '1.21.1')).toBe(
      'HD U J1',
    );
    expect(
      gameLabel({
        versionId: '1.21.10',
        loader: { kind: 'optifine', version: '1.21.10_HD_U_J7_pre11' },
      }),
    ).toContain('OptiFine HD U J7 pre11');
  });

  it('offers OptiFine for installations but not for mods', () => {
    expect(MOD_LOADER_KINDS).not.toContain('optifine');
    expect(MOD_LOADER_KINDS).toContain('neoforge');
  });
});
