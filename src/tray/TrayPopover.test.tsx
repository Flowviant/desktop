// @vitest-environment jsdom
/**
 * THE TRAY CARD, DRIVEN BY THE EVENTS THE RUST SIDE EMITS (SOLID F162).
 *
 * The wiring moved into `useTrayController`; these render the real card in
 * window mode with only the Tauri bridge stood in (`invoke`, `listen`, the
 * window and the folder dialog), and fire the events src-tauri fires: a
 * `status-changed` asks for status again, `update-ready` offers the restart,
 * and the connect flow walks begin-connect → folder → waiting on the browser
 * → bound. A preview render proves fixtures still draw with no bridge at all.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { Snapshot } from '../model';
import { fixtures } from './fixtures';

const WSL_FOLDER = '\\\\wsl.localhost\\Ubuntu\\home\\maya\\checkout-service';
const bridge = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(),
  folder: null as string | null,
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: bridge.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    bridge.handlers.set(name, handler);
    return () => bridge.handlers.delete(name);
  }),
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ isVisible: async () => true }) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => bridge.folder) }));

import { TrayPopover } from './TrayPopover';

let status: Snapshot;

beforeEach(() => {
  bridge.handlers.clear();
  bridge.folder = WSL_FOLDER;
  status = fixtures.serving;
  bridge.invoke.mockReset();
  bridge.invoke.mockImplementation(async (command: string) => {
    if (command === 'status') return status;
    if (command === 'app_version') return '0.0.14';
    if (command === 'pending_update' || command === 'check_update') return null;
    return undefined;
  });
});
afterEach(cleanup);

const emit = async (name: string, payload: unknown = null) => {
  const handler = bridge.handlers.get(name);
  expect(handler, `listening for ${name}`).toBeTruthy();
  await act(async () => { handler!({ payload }); });
};
const statusCalls = () => bridge.invoke.mock.calls.filter(([command]) => command === 'status').length;

async function mountWindow() {
  render(<TrayPopover mode="window" />);
  await screen.findByText('checkout-service');
}

describe('the window card', () => {
  it('asks for status again on status-changed, and renders the answer', async () => {
    await mountWindow();
    const before = statusCalls();
    status = fixtures.inactive;
    await emit('status-changed');
    expect(statusCalls()).toBe(before + 1);
    expect(await screen.findByText("studio-pc is this project's machine")).toBeTruthy();
  });

  // Owner ruling 22 (2026-09-26): a Restart during a drain waits, and says so.
  // The daemon reports what it is finishing (`draining`) and status.rs stamps
  // whether this app's supervisor is standing by to start it again.
  it('says a project is finishing a deploy, and whether it restarts after, even switched off', async () => {
    await mountWindow();
    const row = fixtures.serving.projects[0];
    status = { ...fixtures.serving, projects: [{ ...row, draining: 'web → prod', standingBy: true }] };
    await emit('status-changed');
    expect(await screen.findByText('Finishing a deploy (web → prod) — restarting after')).toBeTruthy();
    // Switched off: the restart is off, the deploy still finishes, and it is still said.
    status = { ...fixtures.serving, projects: [{ ...row, enabled: false, draining: 'web → prod', standingBy: false }] };
    await emit('status-changed');
    expect(await screen.findByText('Finishing a deploy (web → prod)')).toBeTruthy();
    expect(screen.queryByText(/restarting after/)).toBeNull();
  });

  it('offers the restart when update-ready arrives', async () => {
    await mountWindow();
    expect(screen.queryByText(/is ready/)).toBeNull();
    await emit('update-ready', { version: '0.0.15' });
    expect(screen.getByText('Flowviant Machines 0.0.15 is ready')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Restart now' }));
    fireEvent.click(screen.getByRole('button', { name: 'Restart now' }));
    expect(bridge.invoke).toHaveBeenCalledWith('install_update');
  });

  it('walks the connect flow from the folder to bound', async () => {
    await mountWindow();
    await emit('begin-connect');
    expect(bridge.invoke).toHaveBeenCalledWith('connect', { folder: bridge.folder });
    expect(screen.getByText('Connecting…')).toBeTruthy();
    await emit('login-event', { event: 'open_url', code: 'MJBG-JCX75H' });
    expect(screen.getByText('Approve this computer in your browser')).toBeTruthy();
    expect(screen.getByText('MJBG-JCX75H')).toBeTruthy();
    const before = statusCalls();
    await emit('login-event', { event: 'bound' });
    expect(screen.queryByText('Approve this computer in your browser')).toBeNull();
    expect(statusCalls()).toBe(before + 1);
  });

  it('refuses a Windows folder in words, without connecting', async () => {
    bridge.folder = 'C:\\dev\\repo';
    await mountWindow();
    await emit('begin-connect');
    expect(screen.getByRole('alert').textContent).toContain('Windows folders come later');
    expect(bridge.invoke).not.toHaveBeenCalledWith('connect', expect.anything());
  });

  it('a project switch sends set_enabled for that project', async () => {
    await mountWindow();
    await act(async () => {
      fireEvent.click(screen.getByRole('switch', { name: 'checkout-service on this computer' }));
    });
    expect(bridge.invoke).toHaveBeenCalledWith('set_enabled', { id: 'checkout-service', enabled: false });
  });
});

describe('a preview render', () => {
  it('draws a fixture with no bridge calls', () => {
    render(<TrayPopover preview={fixtures.fix} mode="tray" />);
    expect(screen.getByText(/Codex hit its limit · 2 agents parked/)).toBeTruthy();
    expect(bridge.invoke).not.toHaveBeenCalled();
    expect(bridge.handlers.size).toBe(0);
  });
});
