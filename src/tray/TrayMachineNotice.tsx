/**
 * A MACHINE-WIDE FACT, above every project group (frame 2b; split from
 * TrayPopover.tsx 2026-09-26, SOLID F162): the CLI's limit (one login, one
 * limit) and an old daemon. The parked agents are listed under their own
 * projects, each with its own Open, so the notice carries no link — and no
 * callback: it is pure words from the derived `MachineFix`.
 */

import { RuntimeMark } from './TrayParts';
import { limitHeadline, type MachineFix } from './trayModel';

export function TrayMachineNotice({ fix }: { fix: MachineFix }) {
  const amber = fix.kind === 'limit';
  return <div className={`mx-1 mb-[6px] flex flex-col gap-1 rounded-xl p-[10px] ${amber ? 'bg-amber-wash' : 'bg-hover'}`}>
    {fix.kind === 'limit' && <>
      <div className="flex items-center gap-2 text-[13px] font-medium"><RuntimeMark runtime={fix.runtime} /><span>{limitHeadline(fix)}</span></div>
      <p className="m-0 text-xs leading-[1.5]">“{fix.message}”{fix.projects > 1 && ` It's one limit for the whole machine, so it parks agents in ${fix.projects === 2 ? 'both' : `all ${fix.projects}`} projects.`}{fix.runtime !== 'claude' && fix.parked !== 0 && (fix.parked === 1 ? ' It offers to run on Claude instead.' : ' Each one below offers to run on Claude instead.')}</p>
    </>}
    {fix.kind === 'behind' && <><div className="text-[13px] font-medium">flowviant {fix.latest} is out</div><p className="m-0 text-xs leading-[1.5] text-quiet">This machine reports {fix.version}. It updates itself when idle. If it hasn't, restart it.</p></>}
  </div>;
}
