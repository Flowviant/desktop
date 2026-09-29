/**
 * THE ONE FLOWVIANT MACHINES SURFACE (frames 1c, 1d, 1e) — the card's layout.
 *
 * Split 2026-09-26 (SOLID F162) along the reasons each part changes: the
 * machine's plumbing — Tauri commands, event subscriptions, the poll — is
 * `useTrayController.ts`; a machine-wide notice is `TrayMachineNotice.tsx`;
 * one project's section is `TrayProjectGroup.tsx`; the derivations stay in
 * `trayModel.ts`. What is left here is the card itself: header, connect
 * block, update note, the groups in order, the menu and the footer.
 */

import { Button } from '../ui/Button';
import { parseWslPath, type Snapshot } from '../model';
import { fixesFor, headerState, machineFixes, machineLine, projectGroups } from './trayModel';
import { rowButton, Switch } from './TrayParts';
import { TrayMachineNotice } from './TrayMachineNotice';
import { TrayProjectGroup } from './TrayProjectGroup';
import { setupForEmpty, TraySetup } from './TraySetup';
import { useTrayController, type ConnectFlow } from './useTrayController';

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

/** START AT SIGN-IN, in the window only (the tray menu has the same item):
 *  off until pressed, so a new install changes nothing at sign-in until the
 *  person asks (autostart_rules.rs). The window is what a first launch opens. */
function StartAtLogin({ on, onToggle }: { on: boolean; onToggle: (on: boolean) => void }) {
  const label = 'Start Flowviant when I sign in to Windows';
  return <div className="mt-[6px] flex items-start gap-[10px] px-[10px] py-2">
    <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
      <span className="text-[13px]">{label}</span>
      {!on && <span className="text-xs leading-[1.5] text-quiet">Off: projects on this computer start when you open Flowviant Machines.</span>}
    </div>
    <span className="pt-[2px]"><Switch on={on} label={label} onToggle={onToggle} /></span>
  </div>;
}

/** The one Flowviant Machines surface (frames 1c, 1d, 1e). `tray` is the
 *  popover over the notification area; `window` is the same card in the app's
 *  own window, which a launch opens and which runs the connect flow. */
export function TrayPopover(props: { preview?: Snapshot; mode?: 'tray' | 'window'; previewFlow?: ConnectFlow; previewUpdate?: string }) {
  const { preview } = props;
  const {
    windowed, snapshot, visible, flow, update, confirmUpdate, setConfirmUpdate, appVersion, checking,
    switching, displaced, confirmQuit, setConfirmQuit, expanded, setExpanded, notice, now, card, content, setupFlow, startAtLogin,
    connect, checkNow, openProject, setEnabled, start, setupAction, setupCode, reopenConnect, installUpdate, quit, openLogs, setStartAtLogin,
  } = useTrayController(props);

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

  if (!visible && !preview) return null;
  const connectRow = <button className={`${rowButton} py-[6px]`} onClick={() => void connect()}>Connect another project…</button>;
  const body = <div ref={content}>
    <header className="flex flex-col gap-[2px] px-[10px] pb-3 pt-[10px]"><div className="flex items-center gap-2"><span className={`h-2 w-2 rounded-full ${dot === 'ok' ? 'bg-ok' : dot === 'amber' ? 'bg-amber' : 'bg-dim'}`} /><span className="text-sm font-semibold">{boxName}</span></div>{machineLine(snapshot) && <div className="text-xs text-quiet">{machineLine(snapshot)}</div>}</header>
    <ConnectBlock flow={flow} onConnect={() => void connect()} onReopen={reopenConnect} />
    {!snapshot && !flow && <p className="m-0 px-[10px] pb-[10px] text-xs text-quiet" role="status">Looking for projects in WSL…</p>}
    {snapshot && (projects.length === 0 ? (!flow && <>
      {setupForEmpty(snapshot.setup).map((offer, index) => <TraySetup key={`${offer.distro ?? 'wsl'}-${index}`} offer={offer} flow={setupFlow} onRun={(step, distro) => void setupAction(step, distro)} onCode={code => void setupCode(code)} />)}
      {setupForEmpty(snapshot.setup).every(offer => offer.steps.length === 0) && <div className="mx-1 mb-[6px] flex flex-col gap-2 rounded-xl bg-hover p-[10px]">
      <span className="text-[13px] font-medium">No project on this computer yet</span>
      <span className="text-xs leading-[1.5] text-quiet">Pick your repository's folder inside WSL, then approve it in your browser.</span>
      <Button tone="solid" className="self-start" onClick={() => void connect()}>Connect a project</Button>
      </div>}
    </>) : <>
      {update && <div className="mx-1 mb-[6px] flex flex-col gap-1 rounded-xl bg-hover p-[10px]">
        <div className="text-[13px] font-medium">Flowviant Machines {update} is ready</div>
        {confirmUpdate
          ? <><p className="m-0 text-xs leading-[1.5] text-quiet">It closes, updates and opens again. Anything running on this computer stops first.</p>
              <div className="flex gap-2 pt-1"><Button tone="solid" onClick={installUpdate}>Restart now</Button><Button tone="quiet" onClick={() => setConfirmUpdate(false)}>Cancel</Button></div></>
          : <><p className="m-0 text-xs leading-[1.5] text-quiet">It installs itself when nothing is running on this computer.</p>
              <Button className="mt-1 self-start" onClick={() => setConfirmUpdate(true)}>Restart now</Button></>}
      </div>}
      {machine.map((fix, index) => <TrayMachineNotice key={`${fix.kind}-${index}`} fix={fix} />)}
      {groups.map((group, index) => <TrayProjectGroup
        key={group.project.id}
        group={group}
        index={index}
        on={switching[group.project.id] ?? !group.off}
        now={now}
        onToggle={enabled => void setEnabled(group.project.id, enabled)}
        onOpen={(page, agent) => void openProject(group.project.id, page, agent)}
        onStart={() => void start(group.project.id)}
        setup={snapshot.setup}
        setupFlow={setupFlow}
        onSetup={(step, distro) => void setupAction(step, distro)}
        onSetupCode={code => void setupCode(code)}
      />)}
      <div className="mx-[10px] my-[6px] h-px bg-hairline" />
      {confirmQuit ? <div className="px-[10px] py-[6px]"><p className="mt-0 text-xs leading-[1.5] text-quiet">Stops every project on this computer and closes the app. Nothing is lost: opening Flowviant Machines again starts the projects that are on. To pause one project, use its switch instead.</p><div className="flex gap-2"><Button tone="solid" onClick={quit}>Quit</Button><Button tone="quiet" onClick={() => setConfirmQuit(false)}>Cancel</Button></div></div> : <nav className="flex flex-col text-[13px]">{menu('machines', 'Open Machines')}{connectRow}<button className={`${rowButton} py-[6px]`} onClick={openLogs}>Open logs</button><button className={`${rowButton} py-[6px]`} onClick={() => setConfirmQuit(true)}>Quit Flowviant Machines…</button></nav>}
    </>)}
    {windowed && startAtLogin !== null && <StartAtLogin on={startAtLogin} onToggle={on => void setStartAtLogin(on)} />}
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
