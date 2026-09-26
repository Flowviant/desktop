export type Runtime = { id: string; installed: boolean; version: string | null; parkedByLimit?: boolean; message?: string };
// `runtime` is relayed as stored: a runtime this app has no mark for still lists.
export type RemoteAgent = { id: string; name: string; status: string; runtime: string; delivered: number; total: number; asks: boolean; parked: boolean; since: string | null };
export type RemoteProject = { at: string; boxName: string | null; role: 'serving' | 'inactive' | null; servingBoxName: string | null; daemonVersion: string | null; latest: string | null; agents: RemoteAgent[] | null; pressure: { reason: string; at: string } | null };
export type Project = { id: string; name: string | null; dir: string | null; distro: string; daemonVersion?: string | null; stopReason?: string | null; enabled?: boolean; wslStopped?: boolean; remote?: RemoteProject | null; running: boolean | null; holder: string | null; lastPoll: string | null; logFile: string; runtimes: Runtime[] };
export type Snapshot = { schema: number; version: string | null; projects: Project[]; runtimes: Runtime[]; cli: { distro: string; runtimes: Runtime[] }[] };
export type MachineEvent = { event: string; projectId?: string; message?: string; runtime?: string; from?: string; to?: string; code?: string };
export type TrayState = 'serving' | 'idle' | 'attention';

export function parseWslPath(path: string): { distro: string; dir: string } | null {
  path = path.replace(/^\\\\\?\\UNC\\/i, '\\\\');
  const parts = path.split('\\').filter(Boolean);
  if (!path.startsWith('\\\\') || !['wsl$', 'wsl.localhost'].includes(parts[0]?.toLowerCase()) || parts.length < 3) return null;
  if (!/^[A-Za-z0-9._-]+$/.test(parts[1]) || parts.slice(2).some(part => part === '.' || part === '..')) return null;
  return { distro: parts[1], dir: `/${parts.slice(2).join('/')}` };
}

export function rowState(row: Project): 'serving' | 'inactive' | 'displaced' | 'unknown' {
  if (row.holder === 'displaced') return 'displaced';
  if (row.running === false) return 'inactive';
  if (row.running === true && row.holder === 'serving') return 'serving';
  return 'unknown';
}

export function trayState(rows: Project[], attention = false): TrayState {
  if (attention || rows.some(row => rowState(row) === 'displaced')) return 'attention';
  if (rows.some(row => rowState(row) === 'serving')) return 'serving';
  return 'idle';
}

export function reduceEvent(state: TrayState, event: MachineEvent): TrayState {
  if (['displaced', 'stopped-unexpectedly', 'limit-hit'].includes(event.event)) return 'attention';
  if (event.event === 'serving' || event.event === 'limit-cleared') return 'serving';
  if (event.event === 'stopped') return 'idle';
  return state;
}
