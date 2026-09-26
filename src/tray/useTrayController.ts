/**
 * THE TRAY'S WIRING — every Tauri command, event subscription and poll the
 * Flowviant Machines card runs (split from TrayPopover.tsx 2026-09-26, SOLID
 * F162).
 *
 * `TrayPopover` used to own fifteen pieces of state, three event subscriptions,
 * the 30-second poll, the connect / update / on-off commands AND every section
 * of markup, so a change to event timing and a change to a row's words were
 * edits to one 325-line component. This hook is the half a render cannot see:
 * what the machine said (status, machine events, login events, update-ready),
 * when to ask again, and what a press sends to the Rust side. The component
 * renders what it returns; `trayModel.ts` stays the pure derivation.
 *
 * PREVIEW MODE (the dev fixtures) is honoured here, once: every command is a
 * no-op and nothing subscribes, so the same card renders frames 1c/1d/1e
 * outside Tauri without a second code path in the markup.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { parseWslPath, reduceEvent, trayState, type MachineEvent, type Snapshot, type TrayState } from '../model';

/** Connecting a folder: set up flowviant in WSL, then the person approves the
 *  code on the web page the browser opened. Errors are the daemon's own words. */
export type ConnectFlow = { phase: 'starting'; folder: string } | { phase: 'waiting'; code: string | null } | { phase: 'error'; message: string };

export type UpdateCheck = 'idle' | 'checking' | 'current' | 'failed';

export function useTrayController({ preview, mode = 'tray', previewFlow, previewUpdate }: { preview?: Snapshot; mode?: 'tray' | 'window'; previewFlow?: ConnectFlow; previewUpdate?: string }) {
  const windowed = mode === 'window';
  const [snapshot, setSnapshot] = useState<Snapshot | null>(preview ?? null);
  const [visible, setVisible] = useState(Boolean(preview) || windowed);
  const [flow, setFlow] = useState<ConnectFlow | null>(previewFlow ?? null);
  const [tray, setTray] = useState<TrayState>('idle');
  const [update, setUpdate] = useState<string | null>(previewUpdate ?? null);
  const [confirmUpdate, setConfirmUpdate] = useState(false);
  const [appVersion, setAppVersion] = useState<string | null>(preview ? '0.0.11' : null);
  const [checking, setChecking] = useState<UpdateCheck>('idle');
  /** The switch's own answer until the next status arrives (a stop takes a moment). */
  const [switching, setSwitching] = useState<Record<string, boolean>>({});
  const [displaced, setDisplaced] = useState<MachineEvent[]>([]);
  const [confirmQuit, setConfirmQuit] = useState(false);
  const [expanded, setExpanded] = useState<'machines' | 'board' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const card = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const visibleRef = useRef(Boolean(preview));

  // The window is always alive (hidden when closed), so it is also what keeps
  // the tray icon's state current; it asks the server for the remote half only
  // while someone can see it.
  const refresh = useCallback(async () => {
    if (preview) return;
    const shown = windowed ? await getCurrentWindow().isVisible().catch(() => false) : visibleRef.current;
    if (!windowed && !shown) return;
    try {
      const next = await invoke<Snapshot>('status', { remote: shown });
      setSnapshot(next);
      if (windowed) setTray(current => trayState(next.projects, current === 'attention'));
    }
    catch (error) { setNotice(String(error)); }
  }, [preview, windowed]);

  useEffect(() => { if (windowed && !preview) void invoke('set_tray_state', { state: tray }); }, [windowed, preview, tray]);

  const connect = useCallback(async () => {
    if (preview) return;
    if (!windowed) { void invoke('show_main', { connect: true }); return; }
    setNotice(null);
    try {
      const folder = await open({ directory: true, multiple: false, title: 'Choose your repository folder inside WSL' });
      if (!folder || Array.isArray(folder)) return;
      if (!parseWslPath(folder)) { setFlow({ phase: 'error', message: 'Windows folders come later — pick your repository folder inside WSL (it starts with \\\\wsl.localhost\\).' }); return; }
      setFlow({ phase: 'starting', folder });
      await invoke('connect', { folder });
    } catch (error) { setFlow({ phase: 'error', message: String(error) }); }
  }, [preview, windowed]);

  // A newer version of this app, downloaded and waiting (lib.rs's update loop).
  useEffect(() => {
    if (preview) return;
    let active = true;
    void invoke<string>('app_version').then(version => { if (active) setAppVersion(version); }).catch(() => {});
    void invoke<string | null>('pending_update').then(version => { if (active && version) setUpdate(version); }).catch(() => {});
    const off = listen<{ version: string }>('update-ready', event => { if (active) setUpdate(event.payload.version); });
    return () => { active = false; void off.then(stop => stop()); };
  }, [preview]);

  useEffect(() => {
    if (preview || !windowed) return;
    let active = true;
    const offLogin = listen<MachineEvent>('login-event', event => {
      if (!active) return;
      if (event.payload.event === 'open_url') setFlow({ phase: 'waiting', code: event.payload.code ?? null });
      if (event.payload.event === 'error') setFlow({ phase: 'error', message: event.payload.message || 'Could not connect' });
      if (event.payload.event === 'bound') { setFlow(null); void refresh(); }
    });
    const offBegin = listen('begin-connect', () => { if (active) void connect(); });
    const offShown = listen<boolean>('main-visible', () => { if (active) { setNow(Date.now()); void refresh(); } });
    void refresh();
    return () => { active = false; void offLogin.then(off => off()); void offBegin.then(off => off()); void offShown.then(off => off()); };
  }, [preview, windowed, refresh, connect]);

  useEffect(() => {
    if (preview) return;
    let active = true;
    const offVisible = listen<boolean>('tray-visible', event => {
      // The popover's own show/hide; the window is always rendered.
      if (!active || windowed) return;
      visibleRef.current = event.payload;
      setVisible(event.payload);
      if (event.payload) { setNow(Date.now()); void refresh(); }
      else { setConfirmQuit(false); setExpanded(null); }
    });
    const offMachine = listen<MachineEvent>('machine-event', event => {
      if (!active) return;
      if (windowed) setTray(current => reduceEvent(current, event.payload));
      if (event.payload.event === 'displaced' && event.payload.projectId) setDisplaced(current => [...current.filter(item => item.projectId !== event.payload.projectId), event.payload]);
      if (event.payload.event === 'serving' && event.payload.projectId) setDisplaced(current => current.filter(item => item.projectId !== event.payload.projectId));
      if (visibleRef.current || windowed) void refresh();
    });
    const offStatus = listen('status-changed', () => { if (visibleRef.current || windowed) void refresh(); });
    void getCurrentWindow().isVisible().then(shown => { if (active && shown) { visibleRef.current = true; setVisible(true); void refresh(); } });
    return () => { active = false; void offVisible.then(off => off()); void offMachine.then(off => off()); void offStatus.then(off => off()); };
  }, [preview, windowed, refresh]);

  useEffect(() => {
    if (!visible || preview) return;
    void invoke<string | null>('check_update', { force: false }).then(version => { if (version) setUpdate(version); }).catch(() => {});
  }, [visible, preview]);

  async function checkNow() {
    if (preview) return;
    setChecking('checking');
    try {
      const version = await invoke<string | null>('check_update', { force: true });
      if (version) { setUpdate(version); setChecking('idle'); } else setChecking('current');
    } catch { setChecking('failed'); }
  }

  useEffect(() => {
    if (!visible || preview) return;
    const timer = window.setInterval(() => { setNow(Date.now()); void refresh(); }, 30_000);
    return () => window.clearInterval(timer);
  }, [visible, preview, refresh]);

  useEffect(() => {
    if (!visible || windowed) return;
    const frame = requestAnimationFrame(() => card.current?.querySelector('button')?.focus());
    return () => cancelAnimationFrame(frame);
  }, [visible, snapshot]);

  useEffect(() => {
    if (!visible || preview || windowed || !content.current) return;
    const observer = new ResizeObserver(() => { if (content.current) void invoke('fit_tray', { height: Math.ceil(content.current.getBoundingClientRect().height) + 26 }); });
    observer.observe(content.current);
    return () => observer.disconnect();
  }, [visible, preview]);

  useEffect(() => {
    if (preview || windowed) return;
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); void invoke('hide_tray'); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [preview]);

  async function openProject(id: string, page: string, agent?: string) {
    if (preview) return;
    try { await invoke('open_project', { id, page, agent }); if (!windowed) await invoke('hide_tray'); }
    catch (error) { setNotice(String(error)); }
  }

  async function setEnabled(id: string, enabled: boolean) {
    if (preview) return;
    setSwitching(current => ({ ...current, [id]: enabled }));
    try { await invoke('set_enabled', { id, enabled }); await refresh(); }
    catch (error) { setNotice(String(error)); }
    finally { setSwitching(current => { const next = { ...current }; delete next[id]; return next; }); }
  }

  async function start(id: string) {
    if (preview) return;
    try { await invoke('project_action', { id, action: 'start' }); await refresh(); }
    catch (error) { setNotice(String(error)); }
  }

  function reopenConnect(code: string) {
    if (!preview) void invoke('open_connect_page', { code }).catch(error => setNotice(String(error)));
  }

  function installUpdate() {
    if (!preview) void invoke('install_update').catch(error => { setConfirmUpdate(false); setNotice(String(error)); });
  }

  function quit() {
    if (!preview) void invoke('quit_app');
  }

  function openLogs() {
    if (!preview) void invoke('open_logs').catch(error => setNotice(String(error)));
  }

  return {
    windowed, snapshot, visible, flow, update, confirmUpdate, setConfirmUpdate, appVersion, checking,
    switching, displaced, confirmQuit, setConfirmQuit, expanded, setExpanded, notice, now, card, content,
    connect, checkNow, openProject, setEnabled, start, reopenConnect, installUpdate, quit, openLogs,
  };
}
