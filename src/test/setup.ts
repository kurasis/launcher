import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach } from 'vitest';
import { forgetCatalog } from '../features/mods/catalogMemory';

afterEach(() => {
  cleanup();
  clearMocks();
  // The catalog keeps its place for the run; every test starts afresh.
  forgetCatalog();
});
