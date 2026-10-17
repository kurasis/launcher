import { describe, expect, it } from 'vitest';
import { strings } from './strings';

describe('strings', () => {
  it('labels packaged builds as qualification builds', () => {
    expect(strings.buildTag('debug')).toBe('Development build');
    expect(strings.buildTag('release')).toBe('Qualification build');
  });
});
