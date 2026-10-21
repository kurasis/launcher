import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import './styles/tokens.css';
import './styles/global.css';
import { loadLanguage, setLanguage, storedLanguage } from './lib/strings';
import { reportFrontendError } from './lib/ipc';

// Errors in the window's code are kept in the launcher's records (D-061).
window.addEventListener('error', (event) => {
  const error: unknown = event.error;
  reportFrontendError(
    event.message || String(error),
    error instanceof Error ? error.stack : undefined,
  );
});
window.addEventListener('unhandledrejection', (event) => {
  const reason: unknown = event.reason;
  reportFrontendError(
    reason instanceof Error ? reason.message : `Unhandled rejection: ${String(reason)}`,
    reason instanceof Error ? reason.stack : undefined,
  );
});

// A file dropped anywhere but a drop field must not open in the window.
for (const type of ['dragover', 'drop'] as const) {
  window.addEventListener(type, (event) => {
    if (!event.defaultPrevented && event.dataTransfer?.types.includes('Files')) {
      event.preventDefault();
      event.dataTransfer.dropEffect = 'none';
    }
  });
}

const container = document.getElementById('root');
if (!container) {
  throw new Error('Root element #root is missing from index.html');
}

/** Both windows load this bundle; the console window has its own label (D-037). */
function windowLabel(): string {
  try {
    return getCurrentWebviewWindow().label;
  } catch {
    return 'main';
  }
}

// Each window loads only its own code: the main window never needs the
// console's, the console window never the rest of the launcher's.
async function windowContent() {
  if (windowLabel() === 'console') {
    const { ConsoleApp } = await import('./features/console/ConsoleApp');
    return <ConsoleApp />;
  }
  const { App } = await import('./app/App');
  return <App />;
}

// The last chosen language until the core's settings arrive (D-038).
void loadLanguage(storedLanguage())
  .then(setLanguage, () => {
    setLanguage('en');
  })
  .then(windowContent)
  .then((content) => {
    createRoot(container).render(<StrictMode>{content}</StrictMode>);
  });
