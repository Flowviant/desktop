import React from 'react';
import { createRoot } from 'react-dom/client';
import { TrayPopover, type ConnectFlow } from './tray/TrayPopover';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './style.css';

const tauri = '__TAURI_INTERNALS__' in window;
const tray = tauri && getCurrentWindow().label === 'tray';
const params = new URLSearchParams(window.location.search);
const previewView = import.meta.env.DEV && !tauri ? params.get('view') : null;
const root = createRoot(document.getElementById('root')!);

if (tray) {
  document.body.classList.add('tray-window');
  root.render(<React.StrictMode><TrayPopover /></React.StrictMode>);
} else if (previewView === 'tray' || previewView === 'window') {
  // Dev-only preview of frames 1c/1d/1e and the connect states, outside Tauri.
  void import('./tray/fixtures').then(({ fixtures, flows }) => {
    const snapshot = fixtures[params.get('fixture') ?? 'serving'] ?? fixtures.serving;
    const flow: ConnectFlow | undefined = flows[params.get('flow') ?? ''];
    if (previewView === 'tray') document.body.classList.add('tray-preview');
    root.render(<React.StrictMode><TrayPopover preview={snapshot} previewFlow={flow} previewUpdate={params.get('update') ?? undefined} mode={previewView} /></React.StrictMode>);
  });
} else {
  root.render(<React.StrictMode><TrayPopover mode="window" /></React.StrictMode>);
}
