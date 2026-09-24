import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { Button } from './ui/Button';
import { parseWslPath, reduceEvent, rowState, trayState, type MachineEvent, type Project, type Snapshot, type TrayState } from './model';

function heard(value: string | null) {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
}

function ProjectRow({ row, displaced, action, navigate }: { row: Project; displaced: boolean; action: (id: string, verb: string) => void; navigate: (id: string, page: string) => void }) {
  const state = displaced ? 'displaced' : rowState(row);
  const [menu, setMenu] = useState(false);
  return <article className="border-t border-line px-4 py-3">
    <div className="flex items-start justify-between gap-2">
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold">{row.name || row.id}</div>
        {row.dir && <div className="mt-0.5 truncate font-mono text-xs text-quiet" title={row.dir}>{row.dir}</div>}
      </div>
      <span className="shrink-0 rounded-full border border-line px-2 py-0.5 text-xs font-medium">WSL · {row.distro}</span>
    </div>
    <div className="mt-2 flex items-center gap-2 text-xs text-quiet">
      {state !== 'unknown' && <span className={state === 'serving' ? 'text-success' : state === 'displaced' ? 'text-warning' : ''}>{state}</span>}
      {state !== 'unknown' && row.lastPoll && <span>·</span>}
      {heard(row.lastPoll) && <span>Last heard {heard(row.lastPoll)}</span>}
      {row.lastPoll && <span>·</span>}
      {row.daemonVersion && <span>Daemon {row.daemonVersion}</span>}
    </div>
    <div className="mt-2 flex flex-wrap items-center gap-1">
      <Button onClick={() => action(row.id, state === 'serving' ? 'stop' : 'start')}>{state === 'serving' ? 'Stop' : 'Start'}</Button>
      <Button onClick={() => action(row.id, 'restart')}>Restart</Button>
      <Button tone="quiet" onClick={() => navigate(row.id, 'logs')}>Open logs</Button>
      <Button tone="quiet" onClick={() => navigate(row.id, 'board')}>Open Board</Button>
      <Button tone="quiet" onClick={() => navigate(row.id, 'machines')}>Open Machines</Button>
      <div className="relative ml-auto">
        <Button tone="quiet" aria-label={`More actions for ${row.name || row.id}`} aria-expanded={menu} onClick={() => setMenu(!menu)}>⋯</Button>
        {menu && <div className="absolute right-0 top-full z-10 mt-1 w-36 rounded-lg border border-line bg-white p-1 shadow-lg">
          <Button tone="quiet" className="w-full justify-start" onClick={() => { setMenu(false); action(row.id, 'disconnect'); }}>Disconnect</Button>
          <Button tone="quiet" className="w-full justify-start" onClick={() => { setMenu(false); action(row.id, 'forget'); }}>Forget here</Button>
        </div>}
      </div>
    </div>
  </article>;
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [tray, setTray] = useState<TrayState>('idle');
  const [displaced, setDisplaced] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      const result = await invoke<Snapshot>('status');
      setSnapshot(result);
      setTray(current => trayState(result.projects, current === 'attention'));
    } catch (error) { setNotice(String(error)); }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 30_000);
    const offEvent = listen<MachineEvent>('machine-event', event => {
      setTray(current => reduceEvent(current, event.payload));
      if (event.payload.projectId && event.payload.event === 'displaced') setDisplaced(ids => [...new Set([...ids, event.payload.projectId!])]);
      if (event.payload.projectId && event.payload.event === 'serving') setDisplaced(ids => ids.filter(id => id !== event.payload.projectId));
      void refresh();
    });
    const offStatus = listen('status-changed', () => void refresh());
    const offLogin = listen<MachineEvent>('login-event', event => {
      if (event.payload.event === 'open_url' && event.payload.code) setNotice(`Enter code ${event.payload.code} in the browser`);
      if (event.payload.event === 'error') { setNotice(event.payload.message || 'Could not connect'); setBusy(false); }
      if (event.payload.event === 'bound') { setBusy(false); setNotice(null); void refresh(); }
    });
    return () => { window.clearInterval(timer); void offEvent.then(off => off()); void offStatus.then(off => off()); void offLogin.then(off => off()); };
  }, [refresh]);

  useEffect(() => { void invoke('set_tray_state', { state: tray }); }, [tray]);

  async function connect() {
    setNotice(null);
    try {
      const folder = await open({ directory: true, multiple: false, title: 'Choose a project folder inside WSL' });
      if (!folder || Array.isArray(folder)) return;
      if (!parseWslPath(folder)) { setNotice('Windows folders come later — pick a folder inside WSL'); return; }
      setBusy(true);
      await invoke('connect', { folder });
    } catch (error) { setBusy(false); setNotice(String(error)); }
  }

  async function action(id: string, verb: string) {
    if (verb === 'disconnect' && !window.confirm('Disconnect this box from this project? This stops its daemon and removes this box from the project’s Machines list.')) return;
    if (verb === 'forget' && !window.confirm('Forget this project on this box? The project remains in Flowviant.')) return;
    setNotice(null);
    if (verb === 'start' || verb === 'restart' || verb === 'disconnect' || verb === 'forget') setDisplaced(ids => ids.filter(item => item !== id));
    try { await invoke('project_action', { id, action: verb }); await refresh(); }
    catch (error) { setNotice(String(error)); }
  }

  async function navigate(id: string, page: string) {
    try { await invoke('open_project', { id, page }); }
    catch (error) { setNotice(String(error)); }
  }

  const cli = snapshot?.cli.map(group => ({ ...group, runtimes: group.runtimes.filter(rt => rt.id === 'claude' || rt.id === 'codex') })).filter(group => group.runtimes.length > 0) || [];
  const parked = snapshot?.projects.flatMap(project => project.runtimes.filter(rt => rt.parkedByLimit && rt.message).map(rt => ({ id: `${project.id}-${rt.id}`, message: rt.message! }))) || [];
  return <main className="flex min-h-screen flex-col bg-paper">
    <header className="flex items-center justify-between border-b border-line px-4 py-3">
      <h1 className="text-lg font-semibold tracking-tight">Flowviant</h1>
      <span className="font-mono text-xs text-quiet">THIS BOX</span>
    </header>
    <section className="min-h-0 flex-1 overflow-auto">
      {snapshot?.projects.map(row => <ProjectRow key={`${row.distro}-${row.id}`} row={row} displaced={displaced.includes(row.id)} action={action} navigate={navigate} />)}
      <div className="border-t border-line px-4 py-4">
        <Button tone="solid" className="w-full" disabled={busy} onClick={() => void connect()}>{busy ? 'Connecting…' : 'Connect a project'}</Button>
        {notice && <div role="alert" className="mt-2 text-xs text-warning">{notice}</div>}
      </div>
      {(cli.length > 0 || parked.length > 0) && <section className="border-t border-line px-4 py-3">
        <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-quiet">CLI on this box</h2>
        {cli.map(group => <div key={group.distro} className="mb-2">
          <div className="mb-1 font-mono text-xs text-quiet">{group.distro}</div>
          <div className="space-y-1">{group.runtimes.map(rt => <div key={rt.id} className="flex justify-between text-xs"><span className="capitalize">{rt.id}</span><span className="font-mono text-quiet">{rt.installed ? rt.version || 'installed' : 'not installed'}</span></div>)}</div>
        </div>)}
        {parked.map(item => <p key={item.id} className="mt-2 text-xs text-warning">{item.message}</p>)}
      </section>}
    </section>
  </main>;
}
