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

export const flows: Record<string, import('./useTrayController').ConnectFlow> = {
  starting: { phase: 'starting', folder: '\\\\wsl.localhost\\Ubuntu\\home\\maya\\checkout-service' },
  waiting: { phase: 'waiting', code: 'MJBG-JCX75H' },
  error: { phase: 'error', message: 'Connection was not completed: flowviant exited with code 1' },
};

// Frames 2a–2c: one machine, several projects.
const ago = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
const remoteFor = (over: Partial<NonNullable<Project['remote']>>) => ({ ...project.remote!, ...over });
const marketing: Project = { ...project, id: 'marketing-site', name: 'marketing-site', dir: '/home/maya/marketing-site', remote: remoteFor({ agents: [{ id: 'pricing', name: 'pricing-in-euros', status: 'working', runtime: 'claude', delivered: 0, total: 2, asks: false, parked: false, since: ago(3) }] }) };
const docs: Project = { ...project, id: 'docs-portal', name: 'docs-portal', dir: '/home/maya/docs-portal', remote: remoteFor({ agents: [] }) };
fixtures.many = { schema: 1, version: '0.99.2', projects: [docs, marketing, project], runtimes: [], cli: [] };
const codexLimit = [{ id: 'codex', installed: true, version: '1.0.0', parkedByLimit: true, message: "You've reached your usage limit. Try again at 3:40 PM." }];
fixtures.manyFix = { schema: 1, version: '0.97.1', projects: [
  { ...project, daemonVersion: '0.97.1', runtimes: codexLimit, remote: remoteFor({ daemonVersion: '0.97.1', agents: [{ ...agents[2], parked: true }, agents[1]] }) },
  { ...marketing, daemonVersion: '0.97.1', runtimes: codexLimit, remote: remoteFor({ daemonVersion: '0.97.1', agents: [{ id: 'changelog', name: 'changelog-draft', status: 'working', runtime: 'codex', delivered: 0, total: 1, asks: false, parked: true, since: null }] }) },
], runtimes: [], cli: [] };
fixtures.manyInactive = { schema: 1, version: '0.99.2', projects: [
  { ...project, holder: null, remote: remoteFor({ boxName: 'maya-laptop', role: 'inactive', servingBoxName: 'studio-pc', agents: [] }) },
  { ...marketing, remote: remoteFor({ boxName: 'maya-laptop', agents: [{ id: 'pricing', name: 'pricing-in-euros', status: 'working', runtime: 'claude', delivered: 1, total: 2, asks: false, parked: false, since: ago(21) }] }) },
], runtimes: [], cli: [] };

fixtures.withOff = { schema: 1, version: '0.99.2', projects: [
  project,
  { ...docs, enabled: false, running: false },
  { ...marketing, running: false, remote: null, wslStopped: true },
], runtimes: [], cli: [] };
