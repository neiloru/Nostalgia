// Builds dist/server.js (fully bundled: runs without node_modules) and copies the admin UI to dist/admin.
// Also used by scripts/package.mjs.
import { build } from 'esbuild';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export async function bundle(outfile) {
  await build({
    entryPoints: [path.join(root, 'src', 'index.ts')],
    bundle: true,
    platform: 'node',
    format: 'esm',
    target: 'node22',
    outfile,
    // Bundled CommonJS dependencies (express, …) call require() for Node built-ins,
    // which needs a real require inside an ES module.
    banner: { js: "import { createRequire as __nostalgiaRequire } from 'node:module'; const require = __nostalgiaRequire(import.meta.url);" },
    logLevel: 'warning',
  });
}

export function copyAdmin(dir) {
  fs.cpSync(path.join(root, 'admin'), path.join(dir, 'admin'), { recursive: true });
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const dist = path.join(root, 'dist');
  await bundle(path.join(dist, 'server.js'));
  copyAdmin(dist);
  console.log('built dist/server.js and dist/admin');
}
