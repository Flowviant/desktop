/**
 * STRIP COMMENTS WITH THE COMPILER, NOT A REGEX — a TEST helper for the
 * source pins (`model.test.ts`, `trayModel.test.ts`).
 *
 * Origin: the Flowviant app's `@flowviant/shared/test/stripComments`
 * (packages/shared/src/test/stripComments.ts, monorepo commit 9d647189,
 * 2026-09-29), copied when the tray moved into its own repository.
 *
 * A regex eats everything after a `/*` inside a STRING and after `//` inside
 * `'https://…'`. TypeScript's own parser knows what a string, a regex literal
 * and JSX text are; its printer with `removeComments` hands back the same
 * program minus every comment. Nothing in the app bundle imports this (it
 * pulls in the whole compiler).
 */
import ts from 'typescript';

export function stripComments(source: string, fileName = 'source.tsx'): string {
  const kind = /\.tsx$/.test(fileName) ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, false, kind);
  return ts.createPrinter({ removeComments: true }).printFile(sf);
}
