import { describe, expect, it } from 'vitest';
import { instanceFixture } from '../../test/fixtures';
import { installationsFor, inviteBody, parseInvite, validServer } from './invite';

describe('invites', () => {
  it('read back what they write', () => {
    for (const invite of [
      { server: 'play.example.com:25565', version: '1.21.1', loader: null },
      { server: '192.0.2.7:25570', version: '1.20.1', loader: 'forge' as const },
      { server: '[2001:db8::1]:25565', version: '24w14a', loader: 'fabric' as const },
      { server: 'play.example.com:25565', version: '1.21.1', loader: 'optifine' as const },
    ]) {
      expect(parseInvite(inviteBody(invite))).toEqual(invite);
    }
  });

  it('are only messages in the exact form, with a real address', () => {
    for (const body of [
      'hello',
      '🎮 invite play.example.com:25565',
      '🎮 invite play.example.com 1.21.1',
      '🎮 invite play.example.com:0 1.21.1',
      '🎮 invite play.example.com:99999 1.21.1',
      '🎮 invite evil host; --demo 1.21.1',
      '🎮 invite --server:1 1.21.1',
      '🎮 invite play.example.com:25565 1.21.1 liteloader',
      '🎮 invite play.example.com:25565 <b>1.21</b>',
      '🎮 invite play.example.com:25565 1.21.1 fabric extra',
      ' 🎮 invite play.example.com:25565 1.21.1',
    ]) {
      expect(parseInvite(body), body).toBeNull();
    }
    expect(validServer('-bad.example.com:1')).toBe(false);
    expect(validServer('[::1]:25565')).toBe(true);
  });

  it('join with the same version, the same loader first, installed only', () => {
    const vanilla = instanceFixture({ id: 'v', versionId: '1.21.1', health: 'ready' });
    const fabric = instanceFixture({
      id: 'f',
      versionId: '1.21.1',
      health: 'ready',
      loader: { kind: 'fabric', version: '0.16.14' },
    });
    const broken = instanceFixture({ id: 'b', versionId: '1.21.1', health: 'repair_required' });
    const other = instanceFixture({ id: 'o', versionId: '1.20.1', health: 'ready' });
    const invite = { server: 'a.example.com:25565', version: '1.21.1', loader: 'fabric' as const };
    expect(installationsFor(invite, [vanilla, broken, other, fabric]).map((i) => i.id)).toEqual([
      'f',
      'v',
    ]);
  });
});
