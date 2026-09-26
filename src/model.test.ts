import { describe, expect, it } from 'vitest';
import { parseWslPath, reduceEvent, rowState, trayState, type Project } from './model';
import { readFileSync } from 'node:fs';
import { stripComments } from '../../web/src/test/stripComments';

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
  // Which events notify is Rust's alone (`src-tauri/src/notifications.rs`, its
  // own `#[cfg(test)]` cases): the TypeScript `shouldNotify` mirror had no
  // caller, so its test could not see the native behaviour drift. Pinned as
  // an absence, since one coming back passes every other test here. The Rust
  // half (`announce` asks that policy; lib.rs holds no copy) moved into
  // notifications.rs's `the_announcer_asks_this_policy`, where it matches Rust
  // code with comments stripped; this half matches model.ts's code the same way.
  it('keeps the notification policy in one place, the one that notifies', () => {
    const model = stripComments(readFileSync(new URL('./model.ts', import.meta.url), 'utf8'), 'model.ts');
    expect(model).toContain('export function reduceEvent(');
    expect(model).not.toMatch(/shouldNotify/);
  });
});
