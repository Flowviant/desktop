/**
 * The two small things every tray section shares: the row-button class and a
 * CLI's mark (split with TrayPopover.tsx 2026-09-26, SOLID F162). Their own
 * file so the popover, its machine notices and its project groups import one
 * spelling rather than three.
 */

import { ClaudeMark, CodexMark } from '../ui/marks';

export const rowButton = 'w-full rounded-[10px] px-[10px] text-left text-[13px] hover:bg-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent';

export function RuntimeMark({ runtime }: { runtime: string }) {
  if (runtime === 'claude') return <ClaudeMark className="h-[14px] w-[14px] shrink-0 text-claude" />;
  if (runtime === 'codex') return <CodexMark className="h-[14px] w-[14px] shrink-0 text-ink" />;
  return <span className="h-[14px] w-[14px] shrink-0" aria-hidden />;
}
