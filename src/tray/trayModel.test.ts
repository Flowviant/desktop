import { describe, expect, it } from 'vitest';
import { agentLine, deliveredLine, elapsed, fixesFor, headerState, machineFixes, machineLine, olderVersion, projectGroups, projectLine, projectTint } from './trayModel';
import type { Project, RemoteAgent, Snapshot } from '../model';
import { stateWord } from './trayModel';

const agent: RemoteAgent = { id: 'a', name: 'Build', status: 'working', runtime: 'codex', delivered: 0, total: 1, asks: false, parked: true, since: null };
const project: Project = { id: 'p', name: 'Shop', dir: null, distro: 'Ubuntu', running: true, holder: 'serving', lastPoll: null, logFile: '', runtimes: [], daemonVersion: '0.98.0', remote: { at: '', boxName: 'laptop', role: 'serving', servingBoxName: 'laptop', daemonVersion: '0.98.0', latest: '0.99.0', agents: [agent], pressure: null } };
const snapshot = (row: Project): Snapshot => ({ schema: 1, version: '0.98.0', projects: [row], runtimes: [], cli: [] });

describe('tray model', () => {
  it('shows measured serving, attention, and inactive header dots', () => {
    const serving = snapshot({ ...project, remote: { ...project.remote!, latest: null } });
    expect(headerState(serving, fixesFor(serving))).toBe('ok');
    // Behind alone is a note, not a fault; a limit is.
    expect(headerState(snapshot(project), fixesFor(snapshot(project)))).toBe('ok');
    const limited = snapshot({ ...project, runtimes: [{ id: 'codex', installed: true, version: null, parkedByLimit: true, message: 'Wait' }] });
    expect(headerState(limited, fixesFor(limited))).toBe('amber');
    const other = snapshot({ ...project, remote: { ...project.remote!, role: 'inactive', servingBoxName: 'studio', latest: null } });
    expect(fixesFor(other).map(fix => fix.kind)).toEqual(['inactive']);
    expect(headerState(other, fixesFor(other))).toBe('dim');
    const inactive = snapshot({ ...project, running: false, holder: null, remote: null });
    expect(headerState(inactive, [])).toBe('dim');
  });
  it('counts only measured parked agents for a CLI limit', () => {
    const row = { ...project, runtimes: [{ id: 'codex', installed: true, version: null, parkedByLimit: true, message: 'Wait until 3 PM' }] };
    expect(fixesFor(snapshot(row)).find(fix => fix.kind === 'limit')).toEqual({ kind: 'limit', projectId: 'p', runtime: 'codex', message: 'Wait until 3 PM', parked: 1 });
    expect(fixesFor(snapshot({ ...row, remote: null })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: null });
    expect(fixesFor(snapshot({ ...row, remote: { ...project.remote!, agents: [] } })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: 0 });
    expect(fixesFor(snapshot({ ...row, remote: { ...project.remote!, agents: [{ ...agent, parked: true }] } })).find(fix => fix.kind === 'limit')).toMatchObject({ parked: 1 });
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
    // The daemon's own last words ride to the card; none is said as none.
    expect(fixesFor(snapshot({ ...project, running: false, remote: null, stopReason: 'error: this project is not connected to this repository.' })).find(fix => fix.kind === 'not-running')).toMatchObject({ reason: 'error: this project is not connected to this repository.' });
    expect(fixesFor(snapshot({ ...project, running: false, remote: null })).find(fix => fix.kind === 'not-running')).toMatchObject({ reason: null });
  });
  it('formats elapsed time and singular cards, while asks take precedence', () => {
    expect(elapsed('2026-09-25T10:00:00Z', Date.parse('2026-09-25T11:05:30Z'))).toBe('1h 5m');
    expect(elapsed(null, Date.now())).toBeNull();
    expect(deliveredLine(agent)).toBe('0 of 1 card');
    expect(deliveredLine({ ...agent, total: 3 })).toBe('0 of 3 cards');
    expect(agentLine({ ...agent, asks: true })).toBe('asks you a question · Answer →');
    expect(agentLine({ ...agent, asks: true })).toBe('asks you a question · Answer →');
  });

  it('lists limit-parked agents under their own project, and says a park it cannot explain', () => {
    const working = { ...agent, id: 'w', parked: false };
    const row = { ...project, runtimes: [{ id: 'codex', installed: true, version: null, parkedByLimit: true, message: 'Wait' }], remote: { ...project.remote!, agents: [agent, working] } };
    const [group] = projectGroups(snapshot(row), fixesFor(snapshot(row)));
    expect(group.agents.map(a => a.id)).toEqual(['a', 'w']);
    expect(group.limited).toEqual(['codex']);
    expect(projectGroups(snapshot(project), fixesFor(snapshot(project)))[0].limited).toEqual([]);
    expect(stateWord(agent)).toBe('parked');
    expect(stateWord(working)).toBe('working');
  });

  const many = (...rows: Project[]): Snapshot => ({ schema: 1, version: '0.99.0', projects: rows, runtimes: [], cli: [] });
  const other = (id: string, over: Partial<NonNullable<Project['remote']>> = {}): Project => ({ ...project, id, name: id, remote: { ...project.remote!, latest: null, ...over } });

  it('says a CLI limit and an old daemon once for the machine, counting parks across projects', () => {
    const codex = [{ id: 'codex', installed: true, version: null, parkedByLimit: true, message: 'Wait' }];
    const snap = many({ ...other('a', { latest: '0.99.1' }), runtimes: codex }, { ...other('b', { latest: '0.99.1', agents: [{ ...agent, id: 'x' }, { ...agent, id: 'y' }] }), runtimes: codex });
    expect(machineFixes(fixesFor(snap))).toEqual([
      { kind: 'limit', runtime: 'codex', message: 'Wait', parked: 3, projects: 2 },
      { kind: 'behind', latest: '0.99.1', version: '0.98.0' },
    ]);
  });

  it('puts the project that needs you first, then the most recently active, and folds only a measured empty one', () => {
    const asks = { ...agent, id: 'q', parked: false, asks: true, since: null };
    const recent = { ...agent, id: 'r', parked: false, since: '2026-09-25T10:00:00Z' };
    const older = { ...agent, id: 'o', parked: false, since: '2026-09-25T09:00:00Z' };
    const snap = many(other('idle', { agents: [] }), other('unknown', { agents: null }), other('older', { agents: [older] }), other('recent', { agents: [recent] }), other('asks', { agents: [asks] }));
    const groups = projectGroups(snap, fixesFor(snap));
    expect(groups.map(g => g.project.id)).toEqual(['asks', 'recent', 'older', 'idle', 'unknown']);
    expect(groups.find(g => g.project.id === 'idle')!.idle).toBe(true);
    expect(groups.find(g => g.project.id === 'unknown')!.idle).toBe(false);
  });

  it('counts the projects in the header, and keeps one project\'s own line', () => {
    expect(machineLine(many(other('a'), other('b'), other('c')))).toBe('Serving 3 projects · v0.98.0');
    expect(machineLine(many(other('a'), other('b', { role: 'inactive' })))).toBe('Serving 1 of 2 projects · v0.98.0');
    expect(machineLine(many(other('a', { role: 'inactive' }), other('b', { role: 'inactive' })))).toBe('Inactive on 2 projects · v0.98.0');
    expect(machineLine(many(project))).toBe(projectLine(project, '0.99.0'));
    expect(machineLine(many())).toBeNull();
  });

  it('tints a project exactly as the web app does (projectGrad in ProjectRow.tsx)', () => {
    // Values computed with the web function; if one changes, change both.
    expect(projectTint('f5f7db90-27fa-4d27-bd35-b8befc989122').name).toBe('cyan');
    expect(projectTint('0b1c2d3e-aaaa-4bbb-8ccc-123456789abc').name).toBe('lilac');
    expect(projectTint('checkout-service').name).toBe('mint');
  });
});
