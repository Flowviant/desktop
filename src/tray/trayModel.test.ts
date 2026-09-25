import { describe, expect, it } from 'vitest';
import { agentLine, deliveredLine, elapsed, fixesFor, headerState, olderVersion, projectLine } from './trayModel';
import type { Project, RemoteAgent, Snapshot } from '../model';

const agent: RemoteAgent = { id: 'a', name: 'Build', status: 'working', runtime: 'codex', delivered: 0, total: 1, asks: false, parked: true, since: null };
const project: Project = { id: 'p', name: 'Shop', dir: null, distro: 'Ubuntu', running: true, holder: 'serving', lastPoll: null, logFile: '', runtimes: [], daemonVersion: '0.98.0', remote: { at: '', boxName: 'laptop', role: 'serving', servingBoxName: 'laptop', daemonVersion: '0.98.0', latest: '0.99.0', agents: [agent], pressure: null } };
const snapshot = (row: Project): Snapshot => ({ schema: 1, version: '0.98.0', projects: [row], runtimes: [], cli: [] });

describe('tray model', () => {
  it('shows measured serving, attention, and inactive header dots', () => {
    const serving = snapshot({ ...project, remote: { ...project.remote!, latest: null } });
    expect(headerState(serving, fixesFor(serving))).toBe('ok');
    expect(headerState(snapshot(project), fixesFor(snapshot(project)))).toBe('amber');
    const inactive = snapshot({ ...project, running: false, holder: null, remote: null });
    expect(headerState(inactive, [])).toBe('dim');
  });
  it('counts only measured parked agents for a CLI limit', () => {
    const row = { ...project, runtimes: [{ id: 'codex', installed: true, version: null, parkedByLimit: true, message: 'Wait until 3 PM' }] };
    expect(fixesFor(snapshot(row)).find(fix => fix.kind === 'limit')).toEqual({ kind: 'limit', projectId: 'p', runtime: 'codex', message: 'Wait until 3 PM', parked: 1 });
    expect(fixesFor(snapshot({ ...row, remote: null })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: null });
    expect(fixesFor(snapshot({ ...row, remote: { ...project.remote!, agents: [] } })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: 0 });
    expect(fixesFor(snapshot({ ...row, remote: { ...project.remote!, agents: [{ ...agent, parked: 1 }] } })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: 1 });
  });
  it('shows behind only for a strictly older numeric version', () => {
    expect(olderVersion('0.98.0', '0.99.0')).toBe(true);
    expect(olderVersion('0.99.0', '0.99.0')).toBe(false);
    expect(olderVersion('1.0.0', '0.99.0')).toBe(false);
    expect(olderVersion('unknown', '0.99.0')).toBe(false);
    expect(fixesFor(snapshot({ ...project, remote: { ...project.remote!, latest: '0.98.0' } })).some(fix => fix.kind === 'behind')).toBe(false);
  });
  it('shows inactive only when another named box serves', () => {
    const row = { ...project, remote: { ...project.remote!, role: 'inactive' as const, servingBoxName: 'server' } };
    expect(fixesFor(snapshot(row)).some(fix => fix.kind === 'inactive')).toBe(true);
    expect(fixesFor(snapshot({ ...row, remote: { ...row.remote, servingBoxName: 'laptop' } })).some(fix => fix.kind === 'inactive')).toBe(false);
  });
  it('keeps absent and null remote facts unknown, including agents and behind', () => {
    for (const remote of [undefined, null]) {
      const row = { ...project, remote, daemonVersion: null };
      expect(fixesFor(snapshot(row))).toEqual([]);
      expect(projectLine(row, null)).toBe('Serving Shop');
    }
  });
  it('relays displaced messages and shows local not running', () => {
    expect(fixesFor(snapshot(project), [{ event: 'displaced', projectId: 'p', message: 'It moved.' }]).some(fix => fix.kind === 'displaced' && fix.message === 'It moved.')).toBe(true);
    expect(fixesFor(snapshot({ ...project, running: false, remote: null })).some(fix => fix.kind === 'not-running')).toBe(true);
  });
  it('formats elapsed time and singular cards, while asks take precedence', () => {
    expect(elapsed('2026-09-25T10:00:00Z', Date.parse('2026-09-25T11:05:30Z'))).toBe('1h 5m');
    expect(elapsed(null, Date.now())).toBeNull();
    expect(deliveredLine(agent)).toBe('0 of 1 card');
    expect(deliveredLine({ ...agent, total: 3 })).toBe('0 of 3 cards');
    expect(agentLine({ ...agent, asks: true })).toBe('asks you a question · Answer →');
    expect(agentLine({ ...agent, asks: 1 })).toBe('asks you a question · Answer →');
  });
});
