/**
 * The small things every tray section shares: the row-button class, a CLI's
 * mark and the on/off switch (split with TrayPopover.tsx 2026-09-26, SOLID
 * F162). Their own file so the popover, its machine notices and its project
 * groups import one spelling rather than three.
 */

import { ClaudeMark, CodexMark } from '../ui/marks';

export const rowButton = 'w-full rounded-[10px] px-[10px] text-left text-[13px] hover:bg-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent';

/** The one on/off switch: a project's (on this computer or not) and the
 *  window's start-at-sign-in. `label` is its accessible name. */
export function Switch({ on, label, title, onToggle }: { on: boolean; label: string; title?: string; onToggle: (on: boolean) => void }) {
  return <button role="switch" aria-checked={on} aria-label={label} title={title}
    className={`relative h-4 w-[26px] shrink-0 rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${on ? 'bg-ink' : 'bg-[hsl(220_14%_84%)]'}`}
    onClick={() => onToggle(!on)}>
    <span className={`absolute top-[2px] h-3 w-3 rounded-full bg-white shadow-sm transition-[left] ${on ? 'left-3' : 'left-[2px]'}`} />
  </button>;
}

export function RuntimeMark({ runtime }: { runtime: string }) {
  if (runtime === 'claude') return <ClaudeMark className="h-[14px] w-[14px] shrink-0 text-claude" />;
  if (runtime === 'codex') return <CodexMark className="h-[14px] w-[14px] shrink-0 text-ink" />;
  return <span className="h-[14px] w-[14px] shrink-0" aria-hidden />;
}
