import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { TrayPopover } from './tray/TrayPopover';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './style.css';

const tauri = '__TAURI_INTERNALS__' in window;
const tray = tauri && getCurrentWindow().label === 'tray';
const preview = import.meta.env.DEV && !tauri && new URLSearchParams(window.location.search).get('view') === 'tray';
const root = createRoot(document.getElementById('root')!);

if (tray) {
  document.body.classList.add('tray-window');
  root.render(<React.StrictMode><TrayPopover /></React.StrictMode>);
} else if (preview) {
  void import('./tray/fixtures').then(({ fixtures }) => {
    const name = new URLSearchParams(window.location.search).get('fixture') ?? 'serving';
    document.body.classList.add('tray-preview');
    root.render(<React.StrictMode><TrayPopover preview={fixtures[name] ?? fixtures.serving} /></React.StrictMode>);
  });
} else {
  root.render(<React.StrictMode><App /></React.StrictMode>);
}
