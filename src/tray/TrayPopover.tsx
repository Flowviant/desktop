import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { type MachineEvent, type RemoteAgent, type Snapshot } from '../model';
import { Button } from '../ui/Button';
import { ClaudeMark, CodexMark } from '../ui/marks';
import { agentLine, elapsed, fixesFor, headerState, projectLine, type Fix } from './trayModel';

const rowButton = 'w-full rounded-[10px] px-[10px] text-left text-[13px] hover:bg-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent';

function RuntimeMark({ runtime }: { runtime: 'claude' | 'codex' }) {
  return runtime === 'claude' ? <ClaudeMark className="h-[14px] w-[14px] shrink-0 text-claude" /> : <CodexMark className="h-[14px] w-[14px] shrink-0 text-ink" />;
}

export function TrayPopover({ preview }: { preview?: Snapshot }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(preview ?? null);
  const [visible, setVisible] = useState(Boolean(preview));
  const [displaced, setDisplaced] = useState<MachineEvent[]>([]);
  const [confirmQuit, setConfirmQuit] = useState(false);
  const [expanded, setExpanded] = useState<'machines' | 'board' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const card = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const visibleRef = useRef(Boolean(preview));

  const refresh = useCallback(async () => {
    if (preview || !visibleRef.current) return;
    try { setSnapshot(await invoke<Snapshot>('status', { remote: true })); }
    catch (error) { setNotice(String(error)); }
  }, [preview]);

  useEffect(() => {
    if (preview) return;
    let active = true;
    const offVisible = listen<boolean>('tray-visible', event => {
      if (!active) return;
      visibleRef.current = event.payload;
      setVisible(event.payload);
      if (event.payload) { setNow(Date.now()); void refresh(); }
      else { setConfirmQuit(false); setExpanded(null); }
    });
    const offMachine = listen<MachineEvent>('machine-event', event => {
      if (!active) return;
      if (event.payload.event === 'displaced' && event.payload.projectId) setDisplaced(current => [...current.filter(item => item.projectId !== event.payload.projectId), event.payload]);
      if (event.payload.event === 'serving' && event.payload.projectId) setDisplaced(current => current.filter(item => item.projectId !== event.payload.projectId));
      if (visibleRef.current) void refresh();
    });
    const offStatus = listen('status-changed', () => { if (visibleRef.current) void refresh(); });
    void getCurrentWindow().isVisible().then(shown => { if (active && shown) { visibleRef.current = true; setVisible(true); void refresh(); } });
    return () => { active = false; void offVisible.then(off => off()); void offMachine.then(off => off()); void offStatus.then(off => off()); };
  }, [preview, refresh]);

  useEffect(() => {
    if (!visible || preview) return;
    const timer = window.setInterval(() => { setNow(Date.now()); void refresh(); }, 30_000);
    return () => window.clearInterval(timer);
  }, [visible, preview, refresh]);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => card.current?.querySelector('button')?.focus());
    return () => cancelAnimationFrame(frame);
  }, [visible, snapshot]);

  useEffect(() => {
    if (!visible || preview || !content.current) return;
    const observer = new ResizeObserver(() => { if (content.current) void invoke('fit_tray', { height: content.current.getBoundingClientRect().height + 24 }); });
    observer.observe(content.current);
    return () => observer.disconnect();
  }, [visible, preview]);

  useEffect(() => {
    if (preview) return;
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); void invoke('hide_tray'); } };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [preview]);

  async function openProject(id: string, page: string, agent?: string) {
    if (preview) return;
    try { await invoke('open_project', { id, page, agent }); await invoke('hide_tray'); }
    catch (error) { setNotice(String(error)); }
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
  const menu = (page: 'machines' | 'board', label: string) => projects.length === 1
    ? <button className={`${rowButton} py-[6px]`} onClick={() => void openProject(projects[0].id, page)}>{label}</button>
    : <div>
        <button className={`${rowButton} py-[6px]`} aria-expanded={expanded === page} onClick={() => setExpanded(expanded === page ? null : page)}>{label} <span className="float-right">⌄</span></button>
        {expanded === page && projects.map(project => <button key={project.id} className={`${rowButton} py-[6px] pl-5 text-quiet`} onClick={() => void openProject(project.id, page)}>{project.name ?? project.id}</button>)}
      </div>;

  function fixBlock(fix: Fix, index: number) {
    const amber = fix.kind === 'limit' || fix.kind === 'displaced';
    return <div key={`${fix.projectId}-${fix.kind}-${index}`} className={`mx-1 mb-[6px] flex flex-col gap-1 rounded-xl p-[10px] ${amber ? 'bg-amber-wash' : 'bg-hover'}`}>
      {fix.kind === 'limit' && <>
        <div className="flex items-center gap-2 text-[13px] font-medium"><RuntimeMark runtime={fix.runtime} /><span>{fix.runtime === 'claude' ? 'Claude' : 'Codex'} hit its limit{fix.parked === null ? '' : ` · ${fix.parked} ${fix.parked === 1 ? 'agent' : 'agents'} parked`}</span></div>
        <p className="m-0 text-xs leading-[1.5]">“{fix.message}”{fix.runtime !== 'claude' && ' Each parked agent offers to run on Claude instead.'}</p>
        <button className="self-start rounded text-xs font-medium text-amber-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" onClick={() => void openProject(fix.projectId, 'machines')}>Open the parked agents →</button>
      </>}
      {fix.kind === 'displaced' && <p className="m-0 text-xs leading-[1.5]">{fix.message}</p>}
      {fix.kind === 'behind' && <><div className="text-[13px] font-medium">flowviant {fix.latest} is out</div><p className="m-0 text-xs leading-[1.5] text-quiet">This machine reports {fix.version}. It updates itself when idle. If it hasn't, restart it.</p></>}
      {fix.kind === 'inactive' && <>
        <div className="text-[13px] font-medium">{fix.servingBoxName} is this project's machine</div>
        <p className="m-0 text-xs leading-[1.5] text-quiet">This computer is polling but runs nothing. Agents started on {fix.servingBoxName} keep their work there.</p>
        {/* Moving the machine is owner-only and needs a signed-in person; the web Machines page has that action. */}
        <Button className="self-start" onClick={() => void openProject(fix.projectId, 'machines')}>Make it the machine →</Button>
      </>}
      {fix.kind === 'not-running' && <><div className="text-[13px] font-medium">Not running</div><Button className="self-start" onClick={() => void start(fix.projectId)}>Start</Button></>}
    </div>;
  }

  function agentRow(projectId: string, agent: RemoteAgent) {
    const time = agent.status === 'working' ? elapsed(agent.since, now) : null;
    return <button key={agent.id} className={`${rowButton} flex items-center gap-[10px] py-2 ${agent.asks ? 'bg-amber-wash' : ''}`} onClick={() => void openProject(projectId, 'agent', agent.id)}>
      <RuntimeMark runtime={agent.runtime} />
      <span className="flex min-w-0 flex-1 flex-col"><span className="truncate font-medium">{agent.name}</span><span className={`text-xs ${agent.asks ? 'text-amber-ink' : 'text-quiet'}`}>{agent.asks ? <>asks you a question · <strong className="font-medium">Answer →</strong></> : agentLine(agent)}</span></span>
      {!agent.asks && <span className={`shrink-0 text-xs ${agent.status === 'working' ? 'text-accent' : 'text-quiet'}`}>{agent.status}{time && ` · ${time}`}</span>}
    </button>;
  }

  if (!visible && !preview) return null;
  return <div className="w-[352px] p-[6px]"><div ref={card} className="max-h-[calc(100vh-12px)] w-[340px] overflow-y-auto rounded-[14px] bg-white p-[6px] text-ink shadow-[0_12px_40px_hsl(222_30%_20%_/_0.22),0_2px_6px_hsl(222_30%_20%_/_0.08)]"><div ref={content}>
    <header className="flex flex-col gap-[2px] px-[10px] pb-3 pt-[10px]"><div className="flex items-center gap-2"><span className={`h-2 w-2 rounded-full ${dot === 'ok' ? 'bg-ok' : dot === 'amber' ? 'bg-amber' : 'bg-dim'}`} /><span className="text-sm font-semibold">{boxName}</span></div>{projects.map(project => <div key={project.id} className="text-xs text-quiet">{projectLine(project, snapshot?.version ?? null)}</div>)}</header>
    {snapshot && (projects.length === 0 ? <div className="px-[10px] pb-[10px]"><p className="mt-0 text-xs text-quiet">No project on this computer yet.</p><Button tone="solid" onClick={() => { if (!preview) void invoke('show_main'); }}>Connect a project</Button></div> : <>
      {fixes.map(fixBlock)}
      {projects.map(project => project.remote?.agents?.length || project.remote?.pressure ? <section key={project.id} className="flex flex-col">{project.remote?.agents?.map(agent => agentRow(project.id, agent))}{project.remote?.pressure && <div className="px-[10px] py-2 text-xs text-quiet">Waiting for the machine — {project.remote.pressure.reason}.</div>}</section> : null)}
      <div className="mx-[10px] my-[6px] h-px bg-hairline" />
      {confirmQuit ? <div className="px-[10px] py-[6px]"><p className="mt-0 text-xs leading-[1.5] text-quiet">It tears down and exits — anything you type afterwards waits until someone starts it again on that computer.</p><div className="flex gap-2"><Button tone="solid" onClick={() => { if (!preview) void invoke('quit_app'); }}>Quit</Button><Button tone="quiet" onClick={() => setConfirmQuit(false)}>Cancel</Button></div></div> : <nav className="flex flex-col text-[13px]">{menu('machines', 'Open Machines')}{menu('board', 'Open board')}<button className={`${rowButton} py-[6px]`} onClick={() => setConfirmQuit(true)}>Quit Flowviant Machines…</button></nav>}
    </>)}
    {notice && <div role="alert" className="px-[10px] py-2 text-xs text-amber-ink">{notice}</div>}
  </div></div></div>;
}
