// Builds a self-contained server release for the current OS:
//   release/nostalgia-server-<version>-<os>-<arch>/
//     server.js   (fully bundled, no node_modules needed)
//     admin/      (admin web UI)
//     node(.exe)  (the Node.js runtime that runs this script)
//     start.cmd | start.sh, settings.example.yaml, LICENSE, README.txt
// and archives it as .zip (Windows) or .tar.gz (Linux / SteamOS).
//
// Usage: node scripts/package.mjs [--no-runtime]
import { build } from 'esbuild';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(root, '..');
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const withRuntime = !process.argv.includes('--no-runtime');
const win = process.platform === 'win32';
const osName = win ? 'windows' : process.platform === 'linux' ? 'linux' : process.platform;
const name = `nostalgia-server-${pkg.version}-${osName}-${process.arch}`;
const releaseDir = path.join(root, 'release');
const out = path.join(releaseDir, name);

fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });

// 1. Bundle everything (express and yaml included). Bundled CommonJS dependencies call
//    require() for Node built-ins, which needs a real require inside an ES module.
await build({
  entryPoints: [path.join(root, 'src', 'index.ts')],
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node22',
  outfile: path.join(out, 'server.js'),
  banner: { js: "import { createRequire as __nostalgiaRequire } from 'node:module'; const require = __nostalgiaRequire(import.meta.url);" },
  logLevel: 'warning',
});

// 2. Admin UI, example settings, license.
fs.cpSync(path.join(root, 'admin'), path.join(out, 'admin'), { recursive: true });
fs.copyFileSync(path.join(root, 'settings.example.yaml'), path.join(out, 'settings.example.yaml'));
fs.copyFileSync(path.join(repo, 'LICENSE'), path.join(out, 'LICENSE'));

// 3. Node.js runtime.
const nodeName = win ? 'node.exe' : 'node';
if (withRuntime) {
  fs.copyFileSync(process.execPath, path.join(out, nodeName));
  if (!win) fs.chmodSync(path.join(out, nodeName), 0o755);
}

// 4. Start scripts. The settings file lives next to the server; it is created on first start.
if (win) {
  fs.writeFileSync(
    path.join(out, 'start.cmd'),
    [
      '@echo off',
      'cd /d "%~dp0"',
      withRuntime ? '"%~dp0node.exe" server.js --settings settings.yaml %*' : 'node server.js --settings settings.yaml %*',
      'if errorlevel 1 pause',
      '',
    ].join('\r\n'),
  );
} else {
  const sh = path.join(out, 'start.sh');
  fs.writeFileSync(
    sh,
    [
      '#!/usr/bin/env sh',
      'cd "$(dirname "$0")" || exit 1',
      withRuntime ? 'exec ./node server.js --settings settings.yaml "$@"' : 'exec node server.js --settings settings.yaml "$@"',
      '',
    ].join('\n'),
  );
  fs.chmodSync(sh, 0o755);
}

fs.writeFileSync(
  path.join(out, 'README.txt'),
  `Nostalgia server ${pkg.version} (${osName}, ${process.arch})

Start:   ${win ? 'start.cmd' : './start.sh'}
         On first start settings.yaml is created next to this file. Set the admin
         password, libraryDirectory and the IGDB credentials there, then start again.
Admin:   http://<this machine>:8080/admin
Data:    ./data (database and cloud saves) unless dataDirectory says otherwise. Back it up.

${withRuntime ? `Node.js ${process.version} is included (${nodeName}).` : 'Requires Node.js 22.5 or newer on the PATH.'}
Documentation: https://github.com/neiloru/Nostalgia
`.replace(/\n/g, win ? '\r\n' : '\n'),
);

// 5. Archive. bsdtar (Windows 10+) writes zip with -a, GNU tar writes tar.gz.
const archive = path.join(releaseDir, win ? `${name}.zip` : `${name}.tar.gz`);
fs.rmSync(archive, { force: true });
if (win) execFileSync(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe'), ['-a', '-c', '-f', archive, name], { cwd: releaseDir, stdio: 'inherit' });
else execFileSync('tar', ['-czf', archive, name], { cwd: releaseDir, stdio: 'inherit' });
console.log(`${path.relative(repo, archive)} (${(fs.statSync(archive).size / 1048576).toFixed(1)} MB)`);
