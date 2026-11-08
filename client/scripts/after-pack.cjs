// electron-builder afterPack hook: on Linux, put build/linux-launcher.sh in front of the
// Electron binary (renamed to nostalgia-bin). AppImage and tar.gz both start the launcher.
const fs = require('node:fs');
const path = require('node:path');

exports.default = async function afterPack(context) {
  if (context.electronPlatformName !== 'linux') return;
  const name = context.packager.executableName;
  const exe = path.join(context.appOutDir, name);
  const real = path.join(context.appOutDir, 'nostalgia-bin');
  if (!fs.existsSync(exe)) throw new Error(`after-pack: ${exe} not found`);
  fs.renameSync(exe, real);
  fs.chmodSync(real, 0o755);
  const script = fs.readFileSync(path.join(context.packager.info.projectDir, 'build', 'linux-launcher.sh'), 'utf8').replace(/\r\n/g, '\n');
  fs.writeFileSync(exe, script, { mode: 0o755 });
  console.log(`  • after-pack: ${name} -> launcher script, Electron binary -> nostalgia-bin`);
};
