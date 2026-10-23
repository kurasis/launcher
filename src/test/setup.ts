import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach } from 'vitest';
import { forgetFeed } from '../features/feed/feedMemory';
import { forgetCatalog } from '../features/mods/catalogMemory';

afterEach(() => {
  cleanup();
  clearMocks();
  // The catalog and the feed keep their place for the run; every test starts afresh.
  forgetCatalog();
  forgetFeed();
});
