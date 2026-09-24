import { describe, expect, it } from 'vitest';
import { parseWslPath, reduceEvent, rowState, shouldNotify, trayState, type Project } from './model';
import { readFileSync } from 'node:fs';

const row = (running: boolean | null, holder: string | null): Project => ({ id: 'p', name: 'Project', dir: '/home/me/repo', distro: 'Ubuntu', running, holder, lastPoll: null, logFile: '', runtimes: [] });

describe('desktop contract', () => {
  it('parses both WSL UNC hosts and rejects Windows and traversal folders', () => {
    expect(parseWslPath('\\\\wsl$\\Ubuntu-24.04\\home\\me\\a b')).toEqual({ distro: 'Ubuntu-24.04', dir: '/home/me/a b' });
    expect(parseWslPath('\\\\wsl.localhost\\Ubuntu\\home\\me')).toEqual({ distro: 'Ubuntu', dir: '/home/me' });
    expect(parseWslPath('\\\\?\\UNC\\wsl.localhost\\Ubuntu\\home\\me')).toEqual({ distro: 'Ubuntu', dir: '/home/me' });
    expect(parseWslPath('C:\\dev')).toBeNull();
    expect(parseWslPath('\\\\wsl$\\Ubuntu\\home\\..\\secrets')).toBeNull();
  });
  it('keeps unknown distinct from measured inactive', () => {
    expect(rowState(row(null, null))).toBe('unknown');
    expect(rowState(row(false, null))).toBe('inactive');
    expect(rowState(row(true, 'serving'))).toBe('serving');
    expect(rowState(row(true, 'displaced'))).toBe('displaced');
  });
  it('reduces machine events and derives tray state', () => {
    expect(reduceEvent('idle', { event: 'serving' })).toBe('serving');
    expect(reduceEvent('serving', { event: 'limit-hit' })).toBe('attention');
    expect(reduceEvent('attention', { event: 'limit-cleared' })).toBe('serving');
    expect(trayState([row(true, 'serving')])).toBe('serving');
    expect(trayState([row(null, null)])).toBe('idle');
  });
  it('notifies only machine events', () => {
    for (const event of ['displaced', 'stopped-unexpectedly', 'limit-hit', 'update-applied']) expect(shouldNotify({ event })).toBe(true);
    for (const event of ['serving', 'stopped', 'limit-cleared', 'agent-turn', 'card-delivered']) expect(shouldNotify({ event })).toBe(false);
  });
  it('keeps user-facing copy on the binary command', () => {
    const source = readFileSync(new URL('./App.tsx', import.meta.url), 'utf8');
    expect(source).not.toMatch(/npx/i);
  });
});
