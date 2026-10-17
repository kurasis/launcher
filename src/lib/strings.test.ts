import { describe, expect, it } from 'vitest';
import { strings } from './strings';

describe('strings', () => {
  it('labels packaged builds as qualification builds', () => {
    expect(strings.buildTag(true)).toBe('Development build');
    expect(strings.buildTag(false)).toBe('Qualification build');
  });
});
