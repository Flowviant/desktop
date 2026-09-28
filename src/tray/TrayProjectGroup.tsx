/**
 * ONE PROJECT in the tray (frames 2a/2c; split from TrayPopover.tsx
 * 2026-09-26, SOLID F162): its tint square and name, its on/off switch and
 * Board, the facts about it, and the agents it is running.
 *
 * It holds no state and calls no Tauri command. What it shows comes in as the
 * derived `ProjectGroup`; what a press does goes out through three callbacks
 * the controller (`useTrayController`) answers — so the markup and the
 * machine's plumbing change for their own reasons.
 */

import type { RemoteAgent, SetupRoute, SetupStep } from '../model';
import { Button } from '../ui/Button';
import { RuntimeMark, rowButton } from './TrayParts';
import { agentLine, drainLine, elapsed, projectTint, runtimeLabel, stateWord, type Fix, type ProjectGroup } from './trayModel';
import { setupForProject, TraySetup } from './TraySetup';
import type { SetupFlow } from './useTrayController';

export function TrayProjectGroup({ group, index, on, now, onToggle, onOpen, onStart, setup, setupFlow, onSetup, onSetupCode }: {
  group: ProjectGroup;
  index: number;
  /** The switch's position: its own pending answer, else the measured state. */
  on: boolean;
  now: number;
  onToggle: (enabled: boolean) => void;
  onOpen: (page: string, agent?: string) => void;
  onStart: () => void;
  setup?: SetupRoute;
  setupFlow: SetupFlow | null;
  onSetup: (step: SetupStep, distro: string | null) => void;
  onSetupCode: (code: string) => void;
}) {
  const { project } = group;
  const name = project.name ?? project.id;
  const tint = projectTint(project.id);
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
      onClick={() => onToggle(!on)}>
      <span className={`absolute top-[2px] h-3 w-3 rounded-full bg-white shadow-sm transition-[left] ${on ? 'left-3' : 'left-[2px]'}`} />
    </button>
    <button className="-mr-[6px] shrink-0 rounded-md px-[6px] py-px text-xs text-quiet hover:bg-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent" onClick={() => onOpen('board')} aria-label={`Open ${name}'s board`}>Board →</button>
  </div>;
  return <section aria-label={name} className="flex flex-col">
    {index > 0 && <div className="mx-[10px] mt-[6px] h-px bg-[hsl(220_14%_94%)]" />}
    {header}
    {group.fixes.filter(fix => !group.off || fix.kind === 'draining').map((fix, i) => <ProjectNotice key={`${fix.projectId}-${fix.kind}-${i}`} fix={fix} name={name} onOpen={onOpen} onStart={onStart} />)}
    {!group.off && <TraySetup offer={setupForProject(project, setup)} flow={setupFlow} onRun={onSetup} onCode={onSetupCode} />}
    {!group.off && group.agents.map(agent => <AgentRow key={agent.id} agent={agent} limitParked={group.limited.includes(agent.runtime)} now={now} onOpen={onOpen} />)}
    {!group.off && project.remote?.pressure && <div className="px-[10px] py-2 text-xs text-quiet">Waiting for the machine — {project.remote.pressure.reason}.</div>}
  </section>;
}

/** A fact about ONE project, under that project's header. */
function ProjectNotice({ fix, name, onOpen, onStart }: { fix: Fix; name: string; onOpen: (page: string) => void; onStart: () => void }) {
  const amber = fix.kind === 'displaced';
  return <div className={`mx-1 mb-1 flex flex-col gap-2 rounded-xl px-[10px] py-3 ${amber ? 'bg-amber-wash' : 'bg-hover'}`}>
    {fix.kind === 'draining' && <div className="text-[13px] font-medium">{drainLine(fix)}</div>}
    {fix.kind === 'displaced' && <p className="m-0 text-xs leading-[1.5]">{fix.message}</p>}
    {fix.kind === 'inactive' && <>
      <div className="text-[13px] font-medium">{fix.servingBoxName} is this project's machine</div>
      <p className="m-0 text-xs leading-[1.5] text-quiet">This computer polls {name} but runs nothing for it. Agents started on {fix.servingBoxName} keep their work there.</p>
      {/* Moving the machine is owner-only and needs a signed-in person; the web Machines page has that action. */}
      <Button className="self-start" onClick={() => onOpen('machines')}>Make it the machine</Button>
    </>}
    {fix.kind === 'not-running' && <><div className="text-[13px] font-medium">Not running</div>{fix.reason && <p className="m-0 break-words text-xs leading-[1.5] text-quiet">{fix.reason}</p>}<Button className="self-start" onClick={onStart}>Start</Button></>}
  </div>;
}

function AgentRow({ agent, limitParked, now, onOpen }: { agent: RemoteAgent; limitParked: boolean; now: number; onOpen: (page: string, agent?: string) => void }) {
  const time = agent.status === 'working' && !agent.parked ? elapsed(agent.since, now) : null;
  // Parked by a CLI's limit: it says which, and opens the agent page, where
  // "Run on Claude instead" is (2b).
  const byLimit = agent.parked && limitParked;
  const amber = agent.asks || byLimit;
  return <button className={`${rowButton} flex items-center gap-[10px] py-2 ${agent.asks ? 'bg-amber-wash' : ''}`} onClick={() => onOpen('agent', agent.id)}>
    <RuntimeMark runtime={agent.runtime} />
    <span className="flex min-w-0 flex-1 flex-col"><span className="truncate font-medium">{agent.name}</span><span className={`text-xs ${amber ? 'text-amber-ink' : 'text-quiet'}`}>{agent.asks ? <>asks you a question · <strong className="font-medium">Answer →</strong></> : byLimit ? <>parked · {runtimeLabel(agent.runtime)} limit · <strong className="font-medium">Open →</strong></> : agentLine(agent)}</span></span>
    {!agent.asks && !byLimit && <span className={`shrink-0 text-xs ${agent.status === 'working' && !agent.parked ? 'text-accent' : 'text-quiet'}`}>{stateWord(agent)}{time && ` · ${time}`}</span>}
  </button>;
}
