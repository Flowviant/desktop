import { rowState, type MachineEvent, type Project, type RemoteAgent, type Snapshot } from '../model';

export type Fix =
  | { kind: 'limit'; projectId: string; runtime: 'claude' | 'codex'; message: string; parked: number | null }
  | { kind: 'displaced'; projectId: string; message: string }
  | { kind: 'behind'; projectId: string; latest: string; version: string }
  | { kind: 'inactive'; projectId: string; servingBoxName: string }
  | { kind: 'not-running'; projectId: string };

export function olderVersion(version: string, latest: string): boolean {
  const parse = (value: string) => /^v?(\d+)\.(\d+)\.(\d+)(?:[-+][\w.-]+)?$/.exec(value.trim())?.slice(1, 4).map(Number);
  const current = parse(version);
  const next = parse(latest);
  if (!current || !next) return false;
  for (let i = 0; i < 3; i++) {
    if (current[i] !== next[i]) return current[i] < next[i];
  }
  return false;
}

export function fixesFor(snapshot: Snapshot | null, displaced: MachineEvent[] = []): Fix[] {
  if (!snapshot) return [];
  return snapshot.projects.flatMap(project => {
    const fixes: Fix[] = [];
    for (const runtime of project.runtimes) {
      if ((runtime.id === 'claude' || runtime.id === 'codex') && runtime.parkedByLimit && runtime.message) {
        fixes.push({ kind: 'limit', projectId: project.id, runtime: runtime.id, message: runtime.message,
          parked: project.remote?.agents?.filter(agent => agent.parked && agent.runtime === runtime.id).length ?? null });
      }
    }
    for (const event of displaced) {
      if (event.projectId === project.id && event.message) fixes.push({ kind: 'displaced', projectId: project.id, message: event.message });
    }
    const remote = project.remote;
    const version = remote?.daemonVersion ?? project.daemonVersion ?? snapshot.version;
    if (remote?.latest && version && olderVersion(version, remote.latest)) fixes.push({ kind: 'behind', projectId: project.id, latest: remote.latest, version });
    if (remote?.role === 'inactive' && remote.servingBoxName && remote.boxName && remote.servingBoxName !== remote.boxName) {
      fixes.push({ kind: 'inactive', projectId: project.id, servingBoxName: remote.servingBoxName });
    }
    if (project.running === false && remote?.role !== 'inactive') fixes.push({ kind: 'not-running', projectId: project.id });
    return fixes;
  });
}

/** Amber is for what is broken here (a limit, a displaced or stopped daemon).
 *  An update note and "another box is the machine" are facts, not faults. */
const AMBER: Fix['kind'][] = ['limit', 'displaced', 'not-running'];

export function headerState(snapshot: Snapshot | null, fixes: Fix[]): 'ok' | 'amber' | 'dim' {
  if (fixes.some(fix => AMBER.includes(fix.kind))) return 'amber';
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

/** The rows under the fixes. A parked agent whose limit has its own block is
 *  counted there ("2 agents parked") and not listed again as if it worked. */
export function agentRows(project: Project, fixes: Fix[]): RemoteAgent[] {
  const limited = new Set(fixes.filter(fix => fix.kind === 'limit' && fix.projectId === project.id).map(fix => (fix as Extract<Fix, { kind: 'limit' }>).runtime as string));
  return (project.remote?.agents ?? []).filter(agent => !(agent.parked && limited.has(agent.runtime)));
}

/** The right slot's state word: a park outranks the stored status it rides on. */
export function stateWord(agent: RemoteAgent): string {
  return agent.parked ? 'parked' : agent.status;
}
