import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const candidates = [process.env.FLOWVIANT_CLI_DIST, '../../../desk-cli/dist', '../../../cli/dist', '../../cli/dist'].filter(Boolean);
const source = candidates.map(path => resolve(desktop, path)).find(path => existsSync(resolve(path, 'manifest.json')));
const target = resolve(desktop, 'src-tauri/resources/flowviant-linux-x64');
if (source) {
  const manifest = JSON.parse(readFileSync(resolve(source, 'manifest.json'), 'utf8'));
  const file = manifest.files?.['linux-x64'];
  if (!manifest.version || !file?.name || !file?.sha256) throw new Error('CLI manifest lacks linux-x64 or version');
  const from = resolve(source, file.name);
  const digest = createHash('sha256').update(readFileSync(from)).digest('hex');
  if (digest !== file.sha256) throw new Error('CLI linux-x64 checksum differs from manifest');
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(from, target);
  writeFileSync(resolve(dirname(target), 'daemon-version.txt'), `${manifest.version}\n`);
  console.log(`Bundled ${file.name}`);
} else if (!existsSync(target)) {
  throw new Error('Build the CLI release first (see apps/desktop/README.md) or set FLOWVIANT_CLI_DIST.');
}
