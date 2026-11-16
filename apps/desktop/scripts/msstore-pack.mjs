#!/usr/bin/env node
// Packs a Store build of BitSpring into an MSIX for the Microsoft Store. Runs on Windows: it needs
// makeappx.exe and makepri.exe from the Windows SDK (present on GitHub's windows-latest runners).
//
//   npx tauri build --no-bundle --config src-tauri/tauri.msstore.conf.json -- --no-default-features
//   node scripts/msstore-pack.mjs --exe ../../target/release/hifi-desktop.exe --arch x64 --out ../../target/msstore
//
// The package is left unsigned: the Store signs it on submission. To install it locally first,
// sign it with a certificate whose subject equals the manifest's Publisher (the CI workflow does).

import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const tauriDir = resolve(here, '../src-tauri');
const storeDir = join(tauriDir, 'msstore');

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, a, i, all) => (a.startsWith('--') ? [...pairs, [a.slice(2), all[i + 1]]] : pairs), []),
);
const exe = resolve(args.exe ?? '');
const arch = args.arch ?? 'x64';
const out = resolve(args.out ?? join(tauriDir, '../../../target/msstore'));
const identityFile = resolve(args.identity ?? join(storeDir, 'identity.json'));

if (!existsSync(exe)) throw new Error(`--exe ${exe} does not exist; build with tauri build --no-bundle first`);
if (!['x64', 'arm64'].includes(arch)) throw new Error(`--arch must be x64 or arm64, not ${arch}`);

// 0.18.0 → 0.18.0.0. The Store reserves the fourth part, so it has to stay 0.
const { version } = JSON.parse(readFileSync(join(tauriDir, 'tauri.conf.json'), 'utf8'));
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`tauri.conf.json version ${version} is not x.y.z`);
const msixVersion = `${version}.0`;

const identity = JSON.parse(readFileSync(identityFile, 'utf8'));
for (const key of ['name', 'publisher', 'publisherDisplayName']) {
  if (!identity[key]) throw new Error(`${identityFile} has no "${key}"`);
}
const xml = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const manifest = readFileSync(join(storeDir, 'AppxManifest.xml'), 'utf8')
  .replace(/<!--[\s\S]*?-->\s*/, '') // the template's own note
  .replaceAll('{{IDENTITY_NAME}}', xml(identity.name))
  .replaceAll('{{PUBLISHER}}', xml(identity.publisher))
  .replaceAll('{{PUBLISHER_DISPLAY_NAME}}', xml(identity.publisherDisplayName))
  .replaceAll('{{VERSION}}', msixVersion)
  .replaceAll('{{ARCH}}', arch);
if (manifest.includes('{{')) throw new Error('AppxManifest.xml has a placeholder this script does not fill');

// The newest Windows SDK's tools for the machine we run on.
function sdkTool(name) {
  const bin = join(process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)', 'Windows Kits', '10', 'bin');
  const versions = existsSync(bin) ? readdirSync(bin).filter((d) => /^10\./.test(d)).sort(compareVersions).reverse() : [];
  for (const v of versions) {
    const tool = join(bin, v, 'x64', name);
    if (existsSync(tool)) return tool;
  }
  throw new Error(`${name} not found under ${bin}; install the Windows SDK`);
}
function compareVersions(a, b) {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  }
  return 0;
}
const run = (tool, toolArgs) => execFileSync(sdkTool(tool), toolArgs, { stdio: 'inherit' });

// The package's contents: the exe (Tauri embeds the frontend and links WebView2Loader statically,
// so it is the whole app), the manifest and the tile/taskbar icons.
const layout = join(out, `layout-${arch}`);
rmSync(layout, { recursive: true, force: true });
mkdirSync(layout, { recursive: true });
cpSync(exe, join(layout, 'BitSpring.exe'));
cpSync(join(storeDir, 'Assets'), join(layout, 'Assets'), { recursive: true });
writeFileSync(join(layout, 'AppxManifest.xml'), manifest);

// resources.pri lets Windows pick Assets\Square44x44Logo.targetsize-24_altform-unplated.png and
// friends for "Assets\Square44x44Logo.png" in the manifest (taskbar, Start, scale 200%).
const priConfig = join(out, `priconfig-${arch}.xml`);
run('makepri.exe', ['createconfig', '/cf', priConfig, '/dq', 'en-US', '/pv', '10.0.0', '/o']);
run('makepri.exe', ['new', '/pr', layout, '/cf', priConfig, '/mn', join(layout, 'AppxManifest.xml'), '/of', join(layout, 'resources.pri'), '/o']);

const msix = join(out, `BitSpring_${msixVersion}_${arch}.msix`);
run('makeappx.exe', ['pack', '/d', layout, '/p', msix, '/o']);
console.log(`\n${msix}`);
