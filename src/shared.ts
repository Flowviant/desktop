/**
 * THE FIVE RULES THE TRAY SHARES WITH THE FLOWVIANT APP, copied here.
 *
 * Origin: the Flowviant app's private `@flowviant/shared` package (monorepo
 * commit 9d647189, 2026-09-29), copied when this tray moved into its own
 * public repository so it builds without the app:
 *   - `PROJECT_TINT_COUNT`, `projectTintIndex`  ← packages/shared/src/projectTintIndex.ts
 *   - `AgentRuntime`, `isAgentRuntime`, `RUNTIME_LABEL` ← packages/shared/src/schemas/agent-runner.schema.ts
 *   - `versionOlderThan` ← packages/shared/src/schemas/daemonVersions.ts
 *
 * The app keeps the originals. They must stay the same rules on both sides: a
 * project's tint in the tray and on the app's Home, a CLI's name in a
 * sentence, and "is this box behind" all have to agree for the same project,
 * CLI and reported version. A change there is copied here in the same release.
 * This file is the tray's ONE home for each: `trayModel.test.ts` fails when a
 * second copy of any of them appears anywhere else in the tray's source.
 */

/** How many identity tints there are; the app and the tray carry exactly this many. */
export const PROJECT_TINT_COUNT = 4;

/** A project's identity tint, as an index — stable across reorders, creations
 *  and deletions: a function of the id. Order: lilac, cyan, mint, sky. */
export function projectTintIndex(id: string): number {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) | 0;
  return Math.abs(h) % PROJECT_TINT_COUNT;
}

/** The coding CLIs a Flowviant machine drives. */
export const AGENT_RUNTIMES = ['claude', 'codex', 'antigravity'] as const;
export type AgentRuntime = (typeof AGENT_RUNTIMES)[number];
export const isAgentRuntime = (v: unknown): v is AgentRuntime =>
  typeof v === 'string' && (AGENT_RUNTIMES as readonly string[]).includes(v);

/** What to CALL each runtime in a sentence a person reads — the CLI's own
 *  name, never the id. */
export const RUNTIME_LABEL: Record<AgentRuntime, string> = {
  claude: 'Claude',
  codex: 'Codex',
  antigravity: 'Antigravity',
};

/**
 * Is a REPORTED version strictly older than `latest`? The "behind" question.
 *
 * STRICT, and ignorance is NOT behind. Only `[v]MAJOR.MINOR.PATCH` is read,
 * with a `-prerelease` / `+build` tail ignored; anything else — null,
 * `unknown`, a two-part or garbled string — is unmeasured and answers false,
 * so it renders nothing rather than a note built on a guess.
 */
const REPORTED_VERSION = /^v?(\d+)\.(\d+)\.(\d+)(?:[-+][\w.-]+)?$/;
export function versionOlderThan(version: string | null | undefined, latest: string | null | undefined): boolean {
  const parse = (value: string | null | undefined) =>
    value == null ? null : REPORTED_VERSION.exec(value.trim())?.slice(1, 4).map(Number) ?? null;
  const current = parse(version);
  const next = parse(latest);
  if (!current || !next) return false;
  for (let i = 0; i < 3; i++) {
    if (current[i] !== next[i]) return current[i] < next[i];
  }
  return false;
}
