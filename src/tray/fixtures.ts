import type { Project, RemoteAgent, Snapshot } from '../model';

const agents: RemoteAgent[] = [
  { id: 'onboarding-copy', name: 'onboarding-copy', status: 'stuck', runtime: 'claude', delivered: 0, total: 1, asks: true, parked: false, since: null },
  { id: 'billing-webhooks', name: 'billing-webhooks', status: 'working', runtime: 'claude', delivered: 2, total: 3, asks: false, parked: false, since: new Date(Date.now() - 14 * 60_000).toISOString() },
  { id: 'search-reindex', name: 'search-reindex', status: 'working', runtime: 'codex', delivered: 0, total: 1, asks: false, parked: false, since: new Date(Date.now() - 6 * 60_000).toISOString() },
];
const project: Project = { id: 'checkout-service', name: 'checkout-service', dir: '/home/maya/checkout-service', distro: 'Ubuntu', running: true, holder: 'serving', lastPoll: null, logFile: '', daemonVersion: '0.99.2', runtimes: [], remote: { at: '2026-09-25T10:42:00Z', boxName: 'studio-pc', role: 'serving', servingBoxName: 'studio-pc', daemonVersion: '0.99.2', latest: '0.99.2', agents, pressure: null } };

export const fixtures: Record<string, Snapshot> = {
  serving: { schema: 1, version: '0.99.2', projects: [project], runtimes: [], cli: [] },
  fix: { schema: 1, version: '0.97.1', projects: [{ ...project, daemonVersion: '0.97.1', runtimes: [{ id: 'codex', installed: true, version: '1.0.0', parkedByLimit: true, message: "You've reached your usage limit. Try again at 3:40 PM." }], remote: { ...project.remote!, daemonVersion: '0.97.1', agents: agents.filter(agent => agent.id === 'billing-webhooks').concat([{ ...agents[2], parked: true }, { ...agents[2], id: 'another', parked: true }]) } }], runtimes: [], cli: [] },
  inactive: { schema: 1, version: '0.99.2', projects: [{ ...project, running: true, holder: null, remote: { ...project.remote!, boxName: 'maya-laptop', role: 'inactive', servingBoxName: 'studio-pc', agents: [] } }], runtimes: [], cli: [] },
  empty: { schema: 1, version: null, projects: [], runtimes: [], cli: [] },
};
