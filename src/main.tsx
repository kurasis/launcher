import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import './styles/tokens.css';
import './styles/global.css';
import { App } from './app/App';
import { ConsoleApp } from './features/console/ConsoleApp';

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

createRoot(container).render(
  <StrictMode>{windowLabel() === 'console' ? <ConsoleApp /> : <App />}</StrictMode>,
);
