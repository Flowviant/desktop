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
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
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
    if (command === 'app_version') return '0.0.15';
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
  it('offers measured WSL setup and runs the pressed step', async () => {
    status = { ...fixtures.empty, setup: { route: 'wsl', wsl: 'missing', steps: ['installWsl'], distros: [] } };
    render(<TrayPopover mode="window" />);
    expect(await screen.findByRole('button', { name: 'Install WSL' })).toBeTruthy();
    expect(screen.getByText(/Windows asks permission/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Install WSL' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_action', { action: 'installWsl', distro: null });
  });

  it('opens an installed but stopped distro only after a press', async () => {
    status = { ...fixtures.empty, setup: { route: 'wsl', wsl: 'ready', steps: [], distros: [{ name: 'Ubuntu', running: false, claude: { installed: null, signedIn: null, billing: null, subscriptionType: null }, steps: ['openDistro'] }] } };
    render(<TrayPopover mode="window" />);
    expect(await screen.findByText(/On first launch, make a Linux name and password/)).toBeTruthy();
    expect(bridge.invoke).not.toHaveBeenCalledWith('setup_action', expect.anything());
    fireEvent.click(screen.getByRole('button', { name: 'Open Ubuntu' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_action', { action: 'openDistro', distro: 'Ubuntu' });
  });

  it('installs Claude in a running distro and offers browser sign-in after a fresh measurement', async () => {
    status = { ...fixtures.empty, setup: { route: 'wsl', wsl: 'ready', steps: [], distros: [{ name: 'Ubuntu', running: true, claude: { installed: false, signedIn: null, billing: null, subscriptionType: null }, steps: ['installClaude'] }] } };
    render(<TrayPopover mode="window" />);
    expect(await screen.findByRole('button', { name: 'Install Claude Code' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Install Claude Code' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_action', { action: 'installClaude', distro: 'Ubuntu' });
    status = { ...status, setup: { ...status.setup!, distros: [{ name: 'Ubuntu', running: true, claude: { installed: true, signedIn: false, billing: null, subscriptionType: null }, steps: ['signInClaude'] }] } };
    await emit('status-changed');
    expect(await screen.findByRole('button', { name: 'Sign in to Claude' })).toBeTruthy();
    expect(screen.getByText(/browser for Claude subscription/)).toBeTruthy();
    expect(screen.queryByLabelText('Code from browser')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Sign in to Claude' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_action', { action: 'signInClaude', distro: 'Ubuntu' });
    await emit('setup-event', { phase: 'code', action: 'signInClaude', message: 'Paste the code from your browser here.' });
    expect(screen.getByLabelText('Code from browser')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Code from browser'), { target: { value: 'abc123' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send code' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_code', { code: 'abc123' });
  });

  it('re-measures after a setup outcome, including a failed press', async () => {
    status = { ...fixtures.empty, setup: { route: 'wsl', wsl: 'ready', steps: [], distros: [{ name: 'Ubuntu', running: true, claude: { installed: true, signedIn: false, billing: null, subscriptionType: null }, steps: ['signInClaude'] }] } };
    render(<TrayPopover mode="window" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Sign in to Claude' }));
    const before = statusCalls();
    await emit('setup-event', { phase: 'error', action: 'signInClaude', message: 'Claude could not sign in.' });
    await emit('status-changed');
    expect(statusCalls()).toBe(before + 1);
    expect(bridge.invoke).toHaveBeenCalledWith('status', { remote: true, refreshAuth: true });
    expect(screen.getByRole('button', { name: 'Sign in to Claude' })).toBeTruthy();
  });

  it('shows a connected project signed out and removes the step once ready', async () => {
    const project = fixtures.serving.projects[0];
    status = { ...fixtures.serving, projects: [{ ...project, runtimes: [{ id: 'claude', installed: true, version: '2.1.0', signedIn: false }] }] };
    await mountWindow();
    expect(screen.getByRole('button', { name: 'Sign in to Claude' })).toBeTruthy();
    status = { ...status, projects: [{ ...project, runtimes: [{ id: 'claude', installed: true, version: '2.1.0', signedIn: true, billing: 'subscription', subscriptionType: 'max' }] }] };
    await emit('status-changed');
    expect(screen.queryByRole('button', { name: 'Sign in to Claude' })).toBeNull();
    expect(screen.queryByText(/Set up Claude/)).toBeNull();
  });

  it('shows missing Claude on a connected project and names Console billing honestly', async () => {
    const project = fixtures.serving.projects[0];
    status = { ...fixtures.serving, projects: [{ ...project, runtimes: [{ id: 'claude', installed: false, version: null }] }] };
    await mountWindow();
    expect(screen.getByRole('button', { name: 'Install Claude Code' })).toBeTruthy();
    status = { ...status, projects: [{ ...project, runtimes: [{ id: 'claude', installed: true, version: '2.1.0', signedIn: true, billing: 'api', subscriptionType: null }] }] };
    await emit('status-changed');
    expect(screen.queryByRole('button', { name: 'Install Claude Code' })).toBeNull();
    expect(screen.getByText('Claude Code uses Console/API billing on this computer.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Use Claude subscription' }));
    expect(bridge.invoke).toHaveBeenCalledWith('setup_action', { action: 'signInClaude', distro: 'Ubuntu' });
  });

  it('clears the sign-in offer when auth becomes unknown and makes no refusal', async () => {
    status = { ...fixtures.empty, setup: { route: 'wsl', wsl: 'ready', steps: [], distros: [{ name: 'Ubuntu', running: true, claude: { installed: true, signedIn: false, billing: null, subscriptionType: null }, steps: ['signInClaude'] }] } };
    render(<TrayPopover mode="window" />);
    expect(await screen.findByRole('button', { name: 'Sign in to Claude' })).toBeTruthy();
    status = { ...status, setup: { ...status.setup!, distros: [{ name: 'Ubuntu', running: true, claude: { installed: true, signedIn: null, billing: null, subscriptionType: null }, steps: [] }] } };
    await emit('status-changed');
    expect(screen.getByText('No project on this computer yet')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Sign in to Claude' })).toBeNull();
  });

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
    expect(screen.getByRole('alert').textContent).toContain('Flowviant runs inside WSL');
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

// START AT SIGN-IN IS A PERSON'S PRESS (2026-09-29, autostart_rules.rs): the
// window offers it off, a press is what changes Windows, and the switch shows
// what Windows then says. Launch writes nothing (pinned on the Rust side).
describe('start at sign-in', () => {
  const LABEL = 'Start Flowviant when I sign in to Windows';
  /** The sign-in entry as Windows would answer it; a press changes it. */
  function entry(value: boolean | null, refuse?: string) {
    const base = bridge.invoke.getMockImplementation()!;
    bridge.invoke.mockImplementation(async (command: string, args?: { on?: boolean }) => {
      if (command === 'start_at_login') return value;
      if (command === 'set_start_at_login') {
        if (refuse) throw refuse;
        value = args!.on!;
        return value;
      }
      return base(command, args);
    });
  }
  const toggle = () => screen.getByRole('switch', { name: LABEL });

  it('is offered off in the window a first run opens, and only a press turns it on', async () => {
    status = fixtures.empty;
    entry(false);
    render(<TrayPopover mode="window" />);
    await screen.findByText('No project on this computer yet');
    expect((await screen.findByRole('switch', { name: LABEL })).getAttribute('aria-checked')).toBe('false');
    expect(screen.getByText('Off: projects on this computer start when you open Flowviant Machines.')).toBeTruthy();
    expect(bridge.invoke).not.toHaveBeenCalledWith('set_start_at_login', expect.anything());
    await act(async () => { fireEvent.click(toggle()); });
    expect(bridge.invoke).toHaveBeenCalledWith('set_start_at_login', { on: true });
    expect(toggle().getAttribute('aria-checked')).toBe('true');
    expect(screen.queryByText(/^Off: projects/)).toBeNull();
  });

  it('shows an entry an older version made as on, and a press turns it off', async () => {
    entry(true);
    await mountWindow();
    expect((await screen.findByRole('switch', { name: LABEL })).getAttribute('aria-checked')).toBe('true');
    await act(async () => { fireEvent.click(toggle()); });
    expect(bridge.invoke).toHaveBeenCalledWith('set_start_at_login', { on: false });
    expect(toggle().getAttribute('aria-checked')).toBe('false');
  });

  it('follows the tray menu item', async () => {
    entry(false);
    await mountWindow();
    await screen.findByRole('switch', { name: LABEL });
    await emit('start-at-login', true);
    expect(toggle().getAttribute('aria-checked')).toBe('true');
  });

  it('says a refused press in its words and keeps showing what Windows says', async () => {
    entry(false, 'Windows did not change the sign-in entry: access denied');
    await mountWindow();
    await screen.findByRole('switch', { name: LABEL });
    await act(async () => { fireEvent.click(toggle()); });
    expect(screen.getByRole('alert').textContent).toContain('Windows did not change the sign-in entry');
    expect(toggle().getAttribute('aria-checked')).toBe('false');
  });

  it('draws no switch when Windows cannot say', async () => {
    entry(null);
    await mountWindow();
    expect(bridge.invoke).toHaveBeenCalledWith('start_at_login');
    expect(screen.queryByRole('switch', { name: LABEL })).toBeNull();
  });

  it('is the window\'s; the popover leaves it to the tray menu', () => {
    render(<TrayPopover preview={fixtures.empty} mode="window" />);
    expect(toggle().getAttribute('aria-checked')).toBe('false');
    cleanup();
    render(<TrayPopover preview={fixtures.serving} mode="tray" />);
    expect(screen.queryByRole('switch', { name: LABEL })).toBeNull();
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it('wears the tray menu item\'s words', () => {
    const rust = readFileSync(join(__dirname, '../../src-tauri/src/autostart.rs'), 'utf8');
    expect(rust).toContain(`pub(crate) const LABEL: &str = "${LABEL}";`);
    const tray = readFileSync(join(__dirname, '../../src-tauri/src/tray.rs'), 'utf8');
    expect(tray).toMatch(/CheckMenuItem::with_id\(app, "login", autostart::LABEL,/);
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
