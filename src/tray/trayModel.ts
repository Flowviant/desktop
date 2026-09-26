import { RUNTIME_LABEL, isAgentRuntime, projectTintIndex, versionOlderThan } from '@flowviant/shared';
import { rowState, type MachineEvent, type Project, type RemoteAgent, type Snapshot } from '../model';

export type Fix =
  /** `runtime` is the measured CLI id, relayed as reported: a limit belongs to
   *  whichever CLI hit it, including one this app has no mark for. */
  | { kind: 'limit'; projectId: string; runtime: string; message: string; parked: number | null }
  | { kind: 'displaced'; projectId: string; message: string }
  | { kind: 'behind'; projectId: string; latest: string; version: string }
  | { kind: 'inactive'; projectId: string; servingBoxName: string }
  | { kind: 'not-running'; projectId: string; reason: string | null; wslStopped?: boolean };

/** Said for a project whose WSL distro is not running (`wsl --shutdown`). */
export const WSL_STOPPED = "WSL isn't running. This project starts again when WSL does — open a WSL terminal, or press Start.";

/** "Is this box behind?" is the shared `versionOlderThan` (2026-09-26): the
 *  tray's own comparator was a second spelling of a rule the web and API ask
 *  in `@flowviant/shared`. Strict, and an unreadable version is never behind. */

export function fixesFor(snapshot: Snapshot | null, displaced: MachineEvent[] = []): Fix[] {
  if (!snapshot) return [];
  return snapshot.projects.flatMap(project => {
    // A project switched OFF is not a problem to report: it is stopped on purpose.
    if (project.enabled === false) return [];
    const fixes: Fix[] = [];
    // Every CLI the machine reports parked by a limit, by its measured id — no
    // CLI-name branch here (it used to drop Antigravity's limit on the floor).
    for (const runtime of project.runtimes) {
      if (runtime.parkedByLimit && runtime.message) {
        fixes.push({ kind: 'limit', projectId: project.id, runtime: runtime.id, message: runtime.message,
          parked: project.remote?.agents?.filter(agent => agent.parked && agent.runtime === runtime.id).length ?? null });
      }
    }
    for (const event of displaced) {
      if (event.projectId === project.id && event.message) fixes.push({ kind: 'displaced', projectId: project.id, message: event.message });
    }
    const remote = project.remote;
    const version = remote?.daemonVersion ?? project.daemonVersion ?? snapshot.version;
    if (remote?.latest && version && versionOlderThan(version, remote.latest)) fixes.push({ kind: 'behind', projectId: project.id, latest: remote.latest, version });
    if (remote?.role === 'inactive' && remote.servingBoxName && remote.boxName && remote.servingBoxName !== remote.boxName) {
      fixes.push({ kind: 'inactive', projectId: project.id, servingBoxName: remote.servingBoxName });
    }
    if (project.running === false && remote?.role !== 'inactive') fixes.push({ kind: 'not-running', projectId: project.id, reason: project.stopReason ?? (project.wslStopped ? WSL_STOPPED : null), wslStopped: project.wslStopped === true });
    return fixes;
  });
}

/** A CLI's name in a sentence a person reads: the shared `RUNTIME_LABEL`
 *  (one table, so the tray and the web cannot disagree), else the measured id
 *  as reported — a CLI this build has never heard of is still named. */
export function runtimeLabel(id: string): string {
  return isAgentRuntime(id) ? RUNTIME_LABEL[id] : id;
}

/** The limit card's headline: which CLI hit its limit, and — when the machine
 *  measured them — how many agents it parked. */
export function limitHeadline(fix: { runtime: string; parked: number | null }): string {
  const parked = fix.parked === null ? '' : ` · ${fix.parked} ${fix.parked === 1 ? 'agent' : 'agents'} parked`;
  return `${runtimeLabel(fix.runtime)} hit its limit${parked}`;
}

/** Amber is for what is broken here (a limit, a displaced or stopped daemon).
 *  An update note and "another box is the machine" are facts, not faults. */
const AMBER: Fix['kind'][] = ['limit', 'displaced', 'not-running'];

export function headerState(snapshot: Snapshot | null, fixes: Fix[]): 'ok' | 'amber' | 'dim' {
  // WSL shut down on purpose is a state, not a fault: it does not turn the dot amber.
  if (fixes.some(fix => AMBER.includes(fix.kind) && !(fix.kind === 'not-running' && fix.wslStopped))) return 'amber';
  if (snapshot?.projects.some(project => project.remote?.role === 'serving' || (!project.remote?.role && rowState(project) === 'serving'))) return 'ok';
  return 'dim';
}

export function projectLine(project: Project, snapshotVersion: string | null): string {
  const role = project.remote?.role ?? rowState(project);
  const name = project.name ?? project.id;
  const lead = role === 'serving' ? `Serving ${name}` : role === 'inactive' ? `Inactive on ${name}` : role === 'unknown' ? name : `Not running · ${name}`;
  const version = project.daemonVersion ?? snapshotVersion;
  return role === 'serving' || role === 'inactive' ? `${lead}${version ? ` · v${version.replace(/^v/, '')}` : ''}` : lead;
}

export function elapsed(since: string | null, now: number): string | null {
  if (!since) return null;
  const start = Date.parse(since);
  if (!Number.isFinite(start) || start > now) return null;
  const minutes = Math.floor((now - start) / 60_000);
  return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h${minutes % 60 ? ` ${minutes % 60}m` : ''}`;
}

export function deliveredLine(agent: RemoteAgent): string {
  return `${agent.delivered} of ${agent.total} ${agent.total === 1 ? 'card' : 'cards'}${agent.delivered === 0 ? '' : ' delivered'}`;
}

export function agentLine(agent: RemoteAgent): string {
  return agent.asks ? 'asks you a question · Answer →' : deliveredLine(agent);
}

/** The right slot's state word: a park outranks the stored status it rides on. */
export function stateWord(agent: RemoteAgent): string {
  return agent.parked ? 'parked' : agent.status;
}

// ── Several projects on one machine (frames 2a, 2b, 2c) ─────────────────────

/** A project's identity tint. The HASH is the shared `projectTintIndex`, the
 *  same one the web app's `projectGrad` maps onto its `.grad-*` recipes, so a
 *  project wears the same colour on Home and in the tray; these are the tray's
 *  own stops, in the shared order (lilac, cyan, mint, sky). */
const TINTS = [
  { name: 'lilac', from: '#d8caf6', to: '#f6d5ed', angle: 135, ink: '#623DB8' },
  { name: 'cyan', from: '#c0e6f2', to: '#c0ddf6', angle: 135, ink: '#257593' },
  { name: 'mint', from: '#c3eadb', to: '#c0e6f2', angle: 135, ink: '#307E64' },
  { name: 'sky', from: '#c0ddf6', to: '#d8caf6', angle: 160, ink: '#2770B9' },
] as const;
export type Tint = (typeof TINTS)[number];
export function projectTint(id: string): Tint {
  return TINTS[projectTintIndex(id)];
}

/** What belongs to the MACHINE, not a project: a CLI's limit (one login, so one
 *  limit, parking agents wherever they are) and an old daemon. Said once,
 *  above every group. */
export type MachineFix =
  | { kind: 'limit'; runtime: string; message: string; parked: number | null; projects: number }
  | { kind: 'behind'; latest: string; version: string };

export function machineFixes(fixes: Fix[]): MachineFix[] {
  const out: MachineFix[] = [];
  for (const fix of fixes) {
    if (fix.kind === 'limit') {
      const seen = out.find((row): row is Extract<MachineFix, { kind: 'limit' }> => row.kind === 'limit' && row.runtime === fix.runtime);
      if (!seen) { out.push({ kind: 'limit', runtime: fix.runtime, message: fix.message, parked: fix.parked, projects: fix.parked ? 1 : 0 }); continue; }
      if (fix.parked !== null) { seen.parked = (seen.parked ?? 0) + fix.parked; if (fix.parked > 0) seen.projects += 1; }
    }
    if (fix.kind === 'behind' && !out.some(row => row.kind === 'behind')) out.push({ kind: 'behind', latest: fix.latest, version: fix.version });
  }
  return out;
}

export type ProjectGroup = {
  project: Project;
  /** This project's own facts: displaced, inactive, not running. */
  fixes: Fix[];
  /** Every live agent, parked ones included: they are listed under their project. */
  agents: RemoteAgent[];
  /** CLIs whose limit parks agents here (a parked agent names it). */
  limited: string[];
  inactive: boolean;
  /** Measured empty: the server said no agents and nothing needs fixing. */
  idle: boolean;
  /** Switched off on the card: its daemon is stopped on purpose. */
  off: boolean;
};

const PROJECT_FIXES: Fix['kind'][] = ['displaced', 'inactive', 'not-running'];

/** The groups, the project that needs you first, then the most recently active. */
export function projectGroups(snapshot: Snapshot | null, fixes: Fix[]): ProjectGroup[] {
  if (!snapshot) return [];
  const limited = fixes.filter((fix): fix is Extract<Fix, { kind: 'limit' }> => fix.kind === 'limit');
  const groups = snapshot.projects.map(project => {
    const own = fixes.filter(fix => fix.projectId === project.id && PROJECT_FIXES.includes(fix.kind));
    const agents = project.remote?.agents ?? [];
    return {
      project,
      fixes: own,
      agents,
      limited: limited.filter(fix => fix.projectId === project.id).map(fix => fix.runtime),
      inactive: project.enabled !== false && project.remote?.role === 'inactive',
      idle: project.enabled !== false && Array.isArray(project.remote?.agents) && agents.length === 0 && own.length === 0 && !project.remote?.pressure,
      off: project.enabled === false,
    };
  });
  const needsYou = (g: ProjectGroup) => g.agents.some(agent => agent.asks) || g.fixes.some(fix => fix.kind === 'displaced' || fix.kind === 'not-running');
  const byOff = (g: ProjectGroup) => Number(g.off);
  const lastActive = (g: ProjectGroup) => Math.max(0, ...g.agents.map(agent => (agent.since ? Date.parse(agent.since) || 0 : 0)));
  return groups
    .map((group, index) => ({ group, index }))
    .sort((a, b) => byOff(a.group) - byOff(b.group)
      || Number(needsYou(b.group)) - Number(needsYou(a.group))
      || Number(b.group.agents.length > 0) - Number(a.group.agents.length > 0)
      || lastActive(b.group) - lastActive(a.group)
      || a.index - b.index)
    .map(({ group }) => group);
}

/** The header's second line. One project keeps its own line (1c); several are counted (2a, 2c). */
export function machineLine(snapshot: Snapshot | null): string | null {
  const all = snapshot?.projects ?? [];
  if (all.length === 0) return null;
  // A project switched off is not counted: it is not asked to serve.
  const projects = all.filter(project => project.enabled !== false);
  if (projects.length === 0) return all.length === 1 ? 'Off' : `All ${all.length} projects off`;
  if (projects.length === 1) return projectLine(projects[0], snapshot?.version ?? null);
  const role = (project: Project) => project.remote?.role ?? (rowState(project) === 'serving' ? 'serving' : rowState(project) === 'inactive' ? 'inactive' : null);
  const serving = projects.filter(project => role(project) === 'serving').length;
  const inactive = projects.filter(project => role(project) === 'inactive').length;
  const n = projects.length;
  const lead = serving === n ? `Serving ${n} projects`
    : serving > 0 ? `Serving ${serving} of ${n} projects`
    : inactive === n ? `Inactive on ${n} projects`
    : `${n} projects`;
  const version = projects.find(project => project.remote?.daemonVersion)?.remote?.daemonVersion ?? projects.find(project => project.daemonVersion)?.daemonVersion ?? snapshot?.version ?? null;
  return `${lead}${version ? ` · v${version.replace(/^v/, '')}` : ''}`;
}
