import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { PROJECT_TINT_COUNT, projectTintIndex } from '@flowviant/shared';
import { agentLine, deliveredLine, elapsed, fixesFor, headerState, machineFixes, machineLine, projectGroups, projectLine, projectTint, WSL_STOPPED } from './trayModel';
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
    // The rule is the shared `versionOlderThan`; these ask it through the tray.
    const behind = (daemonVersion: string, latest: string) =>
      fixesFor(snapshot({ ...project, remote: { ...project.remote!, daemonVersion, latest } })).some(fix => fix.kind === 'behind');
    expect(behind('0.98.0', '0.99.0')).toBe(true);
    expect(behind('0.99.0', '0.99.0')).toBe(false);
    expect(behind('1.0.0', '0.99.0')).toBe(false);
    expect(behind('unknown', '0.99.0')).toBe(false);
    expect(behind('v0.98.0', '0.99.0')).toBe(true);
    expect(behind('0.98.0-beta.2', '0.99.0')).toBe(true);
    expect(behind('0.99.0-beta.2', '0.99.0')).toBe(false);
    expect(behind('0.98.0 garbled', '0.99.0')).toBe(false);
    expect(fixesFor(snapshot({ ...project, remote: { ...project.remote!, latest: '0.98.0' } })).some(fix => fix.kind === 'behind')).toBe(false);
  });

  it('keeps no comparator of its own', () => {
    // One home for "is this box behind": a local semver parse coming back is
    // the drift this pin exists for. Canary: the file does ask the shared one.
    const src = readFileSync(join(__dirname, 'trayModel.ts'), 'utf8');
    expect(src).toContain('versionOlderThan(');
    expect(src).not.toContain(String.raw`(\d+)\.(\d+)`);
    expect(src).not.toMatch(/function\s+olderVersion/);
  });

  it('relays a limit on ANY CLI the machine names, Antigravity included', () => {
    const agy: RemoteAgent = { ...agent, id: 'g', runtime: 'antigravity', parked: true };
    const row = (id: string, agents: RemoteAgent[]) => ({ ...project, id, runtimes: [{ id: 'antigravity', installed: true, version: null, parkedByLimit: true, message: 'Quota resets at 5 PM' }], remote: { ...project.remote!, latest: null, agents } });
    const snap: Snapshot = { schema: 1, version: '0.99.0', projects: [row('a', [agy]), row('b', [{ ...agy, id: 'h' }, { ...agy, id: 'i' }])], runtimes: [], cli: [] };
    const fixes = fixesFor(snap);
    expect(fixes.filter(fix => fix.kind === 'limit')).toEqual([
      { kind: 'limit', projectId: 'a', runtime: 'antigravity', message: 'Quota resets at 5 PM', parked: 1 },
      { kind: 'limit', projectId: 'b', runtime: 'antigravity', message: 'Quota resets at 5 PM', parked: 2 },
    ]);
    expect(machineFixes(fixes)).toEqual([{ kind: 'limit', runtime: 'antigravity', message: 'Quota resets at 5 PM', parked: 3, projects: 2 }]);
    const groups = projectGroups(snap, fixes);
    expect(groups.map(g => g.limited)).toEqual([['antigravity'], ['antigravity']]);
    expect(headerState(snap, fixes)).toBe('amber');
    // A runtime reporting no limit stays unreported.
    expect(fixesFor(snapshot({ ...project, runtimes: [{ id: 'antigravity', installed: true, version: null }] })).some(fix => fix.kind === 'limit')).toBe(false);
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

  it('tints a project through the shared hash the web app uses (projectTintIndex)', () => {
    expect(projectTint('f5f7db90-27fa-4d27-bd35-b8befc989122').name).toBe('cyan');
    expect(projectTint('0b1c2d3e-aaaa-4bbb-8ccc-123456789abc').name).toBe('lilac');
    expect(projectTint('checkout-service').name).toBe('mint');
    // Every index maps, in the shared order the web's `.grad-*` list keeps.
    const byIndex = new Map<number, string>();
    for (const id of ['f5f7db90-27fa-4d27-bd35-b8befc989122', '0b1c2d3e-aaaa-4bbb-8ccc-123456789abc', 'checkout-service', ...Array.from({ length: 40 }, (_, i) => `p-${i}`)]) {
      byIndex.set(projectTintIndex(id), projectTint(id).name);
    }
    expect(PROJECT_TINT_COUNT).toBe(4);
    expect([0, 1, 2, 3].map(i => byIndex.get(i))).toEqual(['lilac', 'cyan', 'mint', 'sky']);
    // No hash of its own: the multiplier coming back is a second copy.
    expect(readFileSync(join(__dirname, 'trayModel.ts'), 'utf8')).not.toMatch(/\*\s*31\b/);
  });

  it('reports nothing for a project switched off, folds it, and sorts it last', () => {
    const off = { ...other('off', { agents: [] }), running: false, enabled: false, stopReason: 'old reason' };
    const snap = many(off, other('on', { agents: [{ ...agent, id: 'w', parked: false }] }));
    expect(fixesFor(snap).filter(fix => fix.projectId === 'off')).toEqual([]);
    const groups = projectGroups(snap, fixesFor(snap));
    expect(groups.map(g => g.project.id)).toEqual(['on', 'off']);
    expect(groups[1]).toMatchObject({ off: true, idle: false, inactive: false });
  });

  it('says why a project in a stopped WSL distro is not running, without a daemon reason', () => {
    const stopped = { ...project, running: false, remote: null, wslStopped: true };
    expect(fixesFor(snapshot(stopped)).find(fix => fix.kind === 'not-running')).toMatchObject({ reason: WSL_STOPPED });
  });

  it('counts only projects that are on, and a stopped WSL is not amber', () => {
    const off = { ...other('off'), enabled: false };
    expect(machineLine(many(other('a'), other('b'), off))).toBe('Serving 2 projects · v0.98.0');
    expect(machineLine(many({ ...other('a'), enabled: false }, off))).toBe('All 2 projects off');
    const wsl = { ...project, running: false, remote: null, wslStopped: true };
    expect(headerState(snapshot(wsl), fixesFor(snapshot(wsl)))).toBe('dim');
  });
});
