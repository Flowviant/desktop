import { useState } from 'react';
import type { Project, SetupDistro, SetupRoute, SetupStep } from '../model';
import { Button } from '../ui/Button';
import type { SetupFlow } from './useTrayController';

type Offer = { steps: SetupStep[]; distro: string | null; wsl: SetupRoute['wsl']; billing: SetupDistro['claude']['billing'] };

export function setupForProject(project: Project, route?: SetupRoute): Offer {
  const measured = route?.route === 'wsl' ? route.distros.find(row => row.name === project.distro) : null;
  if (measured) return {
    steps: project.wslStopped ? [] : measured.steps,
    distro: project.distro, wsl: route!.wsl, billing: measured.claude.billing,
  };
  const claude = project.runtimes.find(runtime => runtime.id === 'claude');
  const steps: SetupStep[] = project.running === true && claude?.installed === false ? ['installClaude']
    : project.running === true && claude?.installed === true && (claude.signedIn === false || claude.billing === 'api') ? ['signInClaude'] : [];
  return { steps, distro: project.distro, wsl: 'unknown', billing: claude?.billing ?? null };
}

export function setupForEmpty(route?: SetupRoute): Offer[] {
  if (route?.route !== 'wsl') return [];
  if (route.steps.length) return [{ steps: route.steps, distro: null, wsl: route.wsl, billing: null }];
  return route.distros.map(row => ({ steps: row.steps.length ? row.steps : row.running && row.claude.billing === 'api' ? ['signInClaude' as const] : [], distro: row.name, wsl: route.wsl, billing: row.claude.billing }))
    .filter(offer => offer.steps.length > 0 || offer.billing === 'api');
}

function words(step: SetupStep, distro: string | null, wsl: SetupRoute['wsl'], billing: Offer['billing']): { label: string; detail: string } {
  const name = distro ?? 'Ubuntu';
  switch (step) {
    case 'installWsl': return { label: 'Install WSL',
      detail: wsl === 'noDistro'
        ? 'Windows asks permission to install Ubuntu. Restart if asked; Ubuntu then asks for a Linux name and password.'
        : 'Windows asks permission, installs WSL and Ubuntu, then needs a restart. Ubuntu later asks for a Linux name and password.' };
    case 'openDistro': return { label: `Open ${name}`, detail: `Opens ${name}'s window. On first launch, make a Linux name and password there.` };
    case 'installClaude': return { label: 'Install Claude Code', detail: `Downloads Claude Code into ${name}. Install messages appear here.` };
    case 'signInClaude': return billing === 'api'
      ? { label: 'Use Claude subscription', detail: 'Opens your browser to switch from Console/API billing to a Claude subscription. Paste a code here if asked.' }
      : { label: 'Sign in to Claude', detail: 'Opens your browser for Claude subscription sign-in. If Claude asks for a code, paste it here.' };
  }
}

export function TraySetup({ offer, flow, onRun, onCode }: {
  offer: Offer; flow: SetupFlow | null;
  onRun: (step: SetupStep, distro: string | null) => void;
  onCode: (code: string) => void;
}) {
  const [code, setCode] = useState('');
  if (!offer.steps.length && offer.billing !== 'api') return null;
  const current = flow?.distro === offer.distro ? flow : null;
  return <div className="mx-1 mb-1 flex flex-col gap-2 rounded-xl bg-hover p-[10px]">
    {offer.steps.map(step => {
      const copy = words(step, offer.distro, offer.wsl, offer.billing);
      const busy = current?.action === step && ['progress', 'browser', 'code'].includes(current.phase);
      return <div key={step} className="flex flex-col gap-[5px]">
        <span className="text-[13px] font-medium">{copy.label}</span>
        <span className="text-xs leading-[1.5] text-quiet">{copy.detail}</span>
        <Button className="self-start" disabled={busy} onClick={() => onRun(step, offer.distro)}>{copy.label}</Button>
        {current?.action === step && current.message && <p role={current.phase === 'error' ? 'alert' : 'status'} className={`m-0 break-words whitespace-pre-wrap text-xs leading-[1.5] ${current.phase === 'error' ? 'text-amber-ink' : 'text-quiet'}`}>{current.message}</p>}
        {step === 'signInClaude' && current?.action === step && current.phase === 'code' && <form className="flex items-end gap-2" onSubmit={event => { event.preventDefault(); onCode(code); setCode(''); }}>
          <label className="flex min-w-0 flex-1 flex-col gap-1 text-xs">Code from browser
            <input className="w-full rounded-lg border border-line bg-white px-2 py-1.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-accent" value={code} onChange={event => setCode(event.target.value)} autoComplete="off" />
          </label>
          <Button type="submit" disabled={!code.trim()}>Send code</Button>
        </form>}
      </div>;
    })}
    {offer.billing === 'api' && <p className="m-0 text-xs leading-[1.5] text-quiet">Claude Code uses Console/API billing on this computer.</p>}
  </div>;
}
