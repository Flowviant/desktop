import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { parseWslPath, reduceEvent, trayState, type MachineEvent, type RemoteAgent, type Snapshot, type TrayState } from '../model';
import { Button } from '../ui/Button';
import { ClaudeMark, CodexMark } from '../ui/marks';
import { agentLine, elapsed, fixesFor, headerState, limitHeadline, machineFixes, machineLine, projectGroups, projectTint, runtimeLabel, stateWord, type Fix, type MachineFix, type ProjectGroup } from './trayModel';

const rowButton = 'w-full rounded-[10px] px-[10px] text-left text-[13px] hover:bg-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent';

function RuntimeMark({ runtime }: { runtime: string }) {
  if (runtime === 'claude') return <ClaudeMark className="h-[14px] w-[14px] shrink-0 text-claude" />;
  if (runtime === 'codex') return <CodexMark className="h-[14px] w-[14px] shrink-0 text-ink" />;
  return <span className="h-[14px] w-[14px] shrink-0" aria-hidden />;
}

/** Connecting a folder: set up flowviant in WSL, then the person approves the
 *  code on the web page the browser opened. Errors are the daemon's own words. */
export type ConnectFlow = { phase: 'starting'; folder: string } | { phase: 'waiting'; code: string | null } | { phase: 'error'; message: string };

function ConnectBlock({ flow, onConnect, onReopen }: { flow: ConnectFlow | null; onConnect: () => void; onReopen: (code: string) => void }) {
  if (!flow) return null;
  if (flow.phase === 'starting') return <div className="mx-1 mb-[6px] flex flex-col gap-1 rounded-xl bg-hover p-[10px]" role="status">
    <span className="text-[13px] font-medium">Connecting…</span>
    <span className="break-all font-mono text-xs text-quiet">{parseWslPath(flow.folder)?.dir ?? flow.folder}</span>
  </div>;
  if (flow.phase === 'waiting') return <div className="mx-1 mb-[6px] flex flex-col gap-[6px] rounded-xl bg-hover p-[10px]" role="status">
    <span className="text-[13px] font-medium">Approve this computer in your browser</span>
    <span className="text-xs leading-[1.5] text-quiet">Pick the project on the page that opened.{flow.code && <> The code is <span className="font-mono font-medium text-ink">{flow.code}</span>.</>}</span>
    {flow.code && <button className="self-start rounded text-xs font-medium text-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" onClick={() => onReopen(flow.code!)}>Open the page again</button>}
  </div>;
  return <div className="mx-1 mb-[6px] flex flex-col gap-2 rounded-xl bg-amber-wash p-[10px]" role="alert">
    <p className="m-0 break-words text-xs leading-[1.5]">{flow.message}</p>
    <Button className="self-start" onClick={onConnect}>Try again</Button>
  </div>;
}

/** The one Flowviant Machines surface (frames 1c, 1d, 1e). `tray` is the
 *  popover over the notification area; `window` is the same card in the app's
 *  own window, which a launch opens and which runs the connect flow. */
export function TrayPopover({ preview, mode = 'tray', previewFlow, previewUpdate }: { preview?: Snapshot; mode?: 'tray' | 'window'; previewFlow?: ConnectFlow; previewUpdate?: string }) {
  const windowed = mode === 'window';
  const [snapshot, setSnapshot] = useState<Snapshot | null>(preview ?? null);
  const [visible, setVisible] = useState(Boolean(preview) || windowed);
  const [flow, setFlow] = useState<ConnectFlow | null>(previewFlow ?? null);
  const [tray, setTray] = useState<TrayState>('idle');
  const [update, setUpdate] = useState<string | null>(previewUpdate ?? null);
  const [confirmUpdate, setConfirmUpdate] = useState(false);
  const [appVersion, setAppVersion] = useState<string | null>(preview ? '0.0.11' : null);
  const [checking, setChecking] = useState<'idle' | 'checking' | 'current' | 'failed'>('idle');
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

  const projects = snapshot?.projects ?? [];
  const fixes = fixesFor(snapshot, displaced);
  const dot = headerState(snapshot, fixes);
  const boxName = projects.find(project => project.remote?.boxName)?.remote?.boxName ?? 'This computer';
  const groups = projectGroups(snapshot, fixes);
  const machine = machineFixes(fixes);
  const menu = (page: 'machines' | 'board', label: string) => projects.length === 1
    ? <button className={`${rowButton} py-[6px]`} onClick={() => void openProject(projects[0].id, page)}>{label}</button>
    : <div>
        <button className={`${rowButton} py-[6px]`} aria-expanded={expanded === page} onClick={() => setExpanded(expanded === page ? null : page)}>{label} <span className="float-right">⌄</span></button>
        {expanded === page && projects.map(project => <button key={project.id} className={`${rowButton} py-[6px] pl-5 text-quiet`} onClick={() => void openProject(project.id, page)}>{project.name ?? project.id}</button>)}
      </div>;

  /** A machine-wide fact, above every group (2b): the CLI's limit (one login,
   *  one limit) and an old daemon. The parked agents are listed under their
   *  own projects, each with its own Open, so the block carries no link. */
  function machineBlock(fix: MachineFix, index: number) {
    const amber = fix.kind === 'limit';
    return <div key={`${fix.kind}-${index}`} className={`mx-1 mb-[6px] flex flex-col gap-1 rounded-xl p-[10px] ${amber ? 'bg-amber-wash' : 'bg-hover'}`}>
      {fix.kind === 'limit' && <>
        <div className="flex items-center gap-2 text-[13px] font-medium"><RuntimeMark runtime={fix.runtime} /><span>{limitHeadline(fix)}</span></div>
        <p className="m-0 text-xs leading-[1.5]">“{fix.message}”{fix.projects > 1 && ` It's one limit for the whole machine, so it parks agents in ${fix.projects === 2 ? 'both' : `all ${fix.projects}`} projects.`}{fix.runtime !== 'claude' && fix.parked !== 0 && (fix.parked === 1 ? ' It offers to run on Claude instead.' : ' Each one below offers to run on Claude instead.')}</p>
      </>}
      {fix.kind === 'behind' && <><div className="text-[13px] font-medium">flowviant {fix.latest} is out</div><p className="m-0 text-xs leading-[1.5] text-quiet">This machine reports {fix.version}. It updates itself when idle. If it hasn't, restart it.</p></>}
    </div>;
  }

  /** A fact about ONE project, under that project's header. */
  function projectBlock(fix: Fix, index: number, name: string) {
    const amber = fix.kind === 'displaced';
    return <div key={`${fix.projectId}-${fix.kind}-${index}`} className={`mx-1 mb-1 flex flex-col gap-2 rounded-xl px-[10px] py-3 ${amber ? 'bg-amber-wash' : 'bg-hover'}`}>
      {fix.kind === 'displaced' && <p className="m-0 text-xs leading-[1.5]">{fix.message}</p>}
      {fix.kind === 'inactive' && <>
        <div className="text-[13px] font-medium">{fix.servingBoxName} is this project's machine</div>
        <p className="m-0 text-xs leading-[1.5] text-quiet">This computer polls {name} but runs nothing for it. Agents started on {fix.servingBoxName} keep their work there.</p>
        {/* Moving the machine is owner-only and needs a signed-in person; the web Machines page has that action. */}
        <Button className="self-start" onClick={() => void openProject(fix.projectId, 'machines')}>Make it the machine</Button>
      </>}
      {fix.kind === 'not-running' && <><div className="text-[13px] font-medium">Not running</div>{fix.reason && <p className="m-0 break-words text-xs leading-[1.5] text-quiet">{fix.reason}</p>}<Button className="self-start" onClick={() => void start(fix.projectId)}>Start</Button></>}
    </div>;
  }

  /** One project: its tint square and name, what it is running, and its Board. */
  function groupSection(group: ProjectGroup, index: number) {
    const { project } = group;
    const name = project.name ?? project.id;
    const tint = projectTint(project.id);
    const on = switching[project.id] ?? !group.off;
    const folded = group.idle || group.off;
    const header = <div className={`flex items-center gap-[10px] px-[10px] ${folded ? 'py-[10px]' : 'pb-1 pt-[10px]'}`}>
      <span aria-hidden className="flex h-[14px] w-[14px] shrink-0 items-center justify-center rounded-[4px] text-[9px] font-bold" style={{ backgroundImage: `linear-gradient(${tint.angle}deg, ${tint.from}, ${tint.to})`, color: tint.ink }}>{name.slice(0, 1).toUpperCase()}</span>
      <span className={`min-w-0 truncate text-xs font-semibold ${folded || group.inactive ? '' : 'flex-1'} ${group.off ? 'text-quiet' : ''}`}>{name}</span>
      {group.inactive && <span className="min-w-0 flex-1 text-[11px] uppercase tracking-[.05em] text-quiet">inactive</span>}
      {group.off && <span className="min-w-0 flex-1 truncate text-xs text-quiet">off</span>}
      {group.idle && !group.inactive && !group.off && <span className="min-w-0 flex-1 truncate text-xs text-quiet">nothing running</span>}
      {/* ON/OFF PER PROJECT (2026-09-25): off stops this project's daemon and
          keeps it stopped, launch included; on starts it and keeps it running. */}
      <button role="switch" aria-checked={on} aria-label={`${name} on this computer`} title={on ? 'On: this computer serves it' : 'Off: stopped here'}
        className={`relative h-4 w-[26px] shrink-0 rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${on ? 'bg-ink' : 'bg-[hsl(220_14%_84%)]'}`}
        onClick={() => void setEnabled(project.id, !on)}>
        <span className={`absolute top-[2px] h-3 w-3 rounded-full bg-white shadow-sm transition-[left] ${on ? 'left-3' : 'left-[2px]'}`} />
      </button>
      <button className="-mr-[6px] shrink-0 rounded-md px-[6px] py-px text-xs text-quiet hover:bg-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" onClick={() => void openProject(project.id, 'board')} aria-label={`Open ${name}'s board`}>Board →</button>
    </div>;
    return <section key={project.id} aria-label={name} className="flex flex-col">
      {index > 0 && <div className="mx-[10px] mt-[6px] h-px bg-[hsl(220_14%_94%)]" />}
      {header}
      {!group.off && group.fixes.map((fix, i) => projectBlock(fix, i, name))}
      {!group.off && group.agents.map(agent => agentRow(project.id, agent, group.limited.includes(agent.runtime)))}
      {!group.off && project.remote?.pressure && <div className="px-[10px] py-2 text-xs text-quiet">Waiting for the machine — {project.remote.pressure.reason}.</div>}
    </section>;
  }

  function agentRow(projectId: string, agent: RemoteAgent, limitParked = false) {
    const time = agent.status === 'working' && !agent.parked ? elapsed(agent.since, now) : null;
    // Parked by a CLI's limit: it says which, and opens the agent page, where
    // "Run on Claude instead" is (2b).
    const byLimit = agent.parked && limitParked;
    const amber = agent.asks || byLimit;
    return <button key={agent.id} className={`${rowButton} flex items-center gap-[10px] py-2 ${agent.asks ? 'bg-amber-wash' : ''}`} onClick={() => void openProject(projectId, 'agent', agent.id)}>
      <RuntimeMark runtime={agent.runtime} />
      <span className="flex min-w-0 flex-1 flex-col"><span className="truncate font-medium">{agent.name}</span><span className={`text-xs ${amber ? 'text-amber-ink' : 'text-quiet'}`}>{agent.asks ? <>asks you a question · <strong className="font-medium">Answer →</strong></> : byLimit ? <>parked · {runtimeLabel(agent.runtime)} limit · <strong className="font-medium">Open →</strong></> : agentLine(agent)}</span></span>
      {!agent.asks && !byLimit && <span className={`shrink-0 text-xs ${agent.status === 'working' && !agent.parked ? 'text-accent' : 'text-quiet'}`}>{stateWord(agent)}{time && ` · ${time}`}</span>}
    </button>;
  }

  if (!visible && !preview) return null;
  const connectRow = <button className={`${rowButton} py-[6px]`} onClick={() => void connect()}>Connect another project…</button>;
  const body = <div ref={content}>
    <header className="flex flex-col gap-[2px] px-[10px] pb-3 pt-[10px]"><div className="flex items-center gap-2"><span className={`h-2 w-2 rounded-full ${dot === 'ok' ? 'bg-ok' : dot === 'amber' ? 'bg-amber' : 'bg-dim'}`} /><span className="text-sm font-semibold">{boxName}</span></div>{machineLine(snapshot) && <div className="text-xs text-quiet">{machineLine(snapshot)}</div>}</header>
    <ConnectBlock flow={flow} onConnect={() => void connect()} onReopen={code => { if (!preview) void invoke('open_connect_page', { code }).catch(error => setNotice(String(error))); }} />
    {!snapshot && !flow && <p className="m-0 px-[10px] pb-[10px] text-xs text-quiet" role="status">Looking for projects in WSL…</p>}
    {snapshot && (projects.length === 0 ? (!flow && <div className="mx-1 mb-[6px] flex flex-col gap-2 rounded-xl bg-hover p-[10px]">
      <span className="text-[13px] font-medium">No project on this computer yet</span>
      <span className="text-xs leading-[1.5] text-quiet">Pick your repository's folder inside WSL, then approve it in your browser.</span>
      <Button tone="solid" className="self-start" onClick={() => void connect()}>Connect a project</Button>
    </div>) : <>
      {update && <div className="mx-1 mb-[6px] flex flex-col gap-1 rounded-xl bg-hover p-[10px]">
        <div className="text-[13px] font-medium">Flowviant Machines {update} is ready</div>
        {confirmUpdate
          ? <><p className="m-0 text-xs leading-[1.5] text-quiet">It closes, updates and opens again. Anything running on this computer stops first.</p>
              <div className="flex gap-2 pt-1"><Button tone="solid" onClick={() => { if (!preview) void invoke('install_update').catch(error => { setConfirmUpdate(false); setNotice(String(error)); }); }}>Restart now</Button><Button tone="quiet" onClick={() => setConfirmUpdate(false)}>Cancel</Button></div></>
          : <><p className="m-0 text-xs leading-[1.5] text-quiet">It installs itself when nothing is running on this computer.</p>
              <Button className="mt-1 self-start" onClick={() => setConfirmUpdate(true)}>Restart now</Button></>}
      </div>}
      {machine.map(machineBlock)}
      {groups.map(groupSection)}
      <div className="mx-[10px] my-[6px] h-px bg-hairline" />
      {confirmQuit ? <div className="px-[10px] py-[6px]"><p className="mt-0 text-xs leading-[1.5] text-quiet">Stops every project on this computer and closes the app. Nothing is lost: opening Flowviant Machines again starts the projects that are on. To pause one project, use its switch instead.</p><div className="flex gap-2"><Button tone="solid" onClick={() => { if (!preview) void invoke('quit_app'); }}>Quit</Button><Button tone="quiet" onClick={() => setConfirmQuit(false)}>Cancel</Button></div></div> : <nav className="flex flex-col text-[13px]">{menu('machines', 'Open Machines')}{connectRow}<button className={`${rowButton} py-[6px]`} onClick={() => { if (!preview) void invoke('open_logs').catch(error => setNotice(String(error))); }}>Open logs</button><button className={`${rowButton} py-[6px]`} onClick={() => setConfirmQuit(true)}>Quit Flowviant Machines…</button></nav>}
    </>)}
    {notice && <div role="alert" className="px-[10px] py-2 text-xs text-amber-ink">{notice}</div>}
    {appVersion && <div className="flex items-center justify-between gap-2 px-[10px] pb-1 pt-2 text-[11px] text-quiet">
      <span>Flowviant Machines {appVersion}</span>
      {!update && <button className="rounded px-1 hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:opacity-60" disabled={checking === 'checking'} onClick={() => void checkNow()}>
        {checking === 'checking' ? 'Checking…' : checking === 'current' ? 'Up to date' : checking === 'failed' ? 'Could not check — try again' : 'Check for updates'}
      </button>}
    </div>}
  </div>;
  if (windowed) return <div ref={card} className="min-h-screen bg-white p-[6px] text-ink">{body}</div>;
  // The rounded card CLIPS and an inner box scrolls: a scrollbar drawn on the
  // rounded element itself squared off the card's right corners.
  return <div className="w-[352px] p-[6px]"><div ref={card} className="w-[340px] overflow-hidden rounded-[14px] bg-white text-ink shadow-[0_12px_40px_hsl(222_30%_20%_/_0.22),0_2px_6px_hsl(222_30%_20%_/_0.08)]"><div className="fv-scroll max-h-[calc(100vh-12px)] overflow-y-auto p-[6px]">{body}</div></div></div>;
}
