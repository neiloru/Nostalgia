// Renders build/icon.svg to PNG (all sizes) and a multi-size Windows .ico.
// Run with:  npx electron scripts/render-icon.cjs
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const svg = fs.readFileSync(path.join(root, 'build', 'icon.svg'), 'utf8');

async function render(win, size) {
  const html = `<html><body style="margin:0;background:transparent">${svg.replace('<svg ', `<svg style="display:block;width:${size}px;height:${size}px" `)}</body></html>`;
  win.setContentSize(size, size);
  await win.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(html));
  await new Promise((r) => setTimeout(r, 150));
  const img = await win.webContents.capturePage({ x: 0, y: 0, width: size, height: size });
  return img.resize({ width: size, height: size, quality: 'best' }).toPNG();
}

function ico(pngs) {
  // ICO container with PNG-compressed entries (supported since Windows Vista).
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(pngs.length, 4);
  const dir = Buffer.alloc(16 * pngs.length);
  let offset = 6 + dir.length;
  pngs.forEach(({ size, data }, i) => {
    const o = i * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, o);
    dir.writeUInt8(size >= 256 ? 0 : size, o + 1);
    dir.writeUInt8(0, o + 2);
    dir.writeUInt8(0, o + 3);
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(data.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += data.length;
  });
  return Buffer.concat([header, dir, ...pngs.map((p) => p.data)]);
}

app.disableHardwareAcceleration();
app.whenReady().then(async () => {
  const win = new BrowserWindow({ show: false, transparent: true, frame: false, useContentSize: true, webPreferences: { offscreen: true } });
  const big = await render(win, 1024);
  fs.writeFileSync(path.join(root, 'build', 'icon.png'), big);
  fs.copyFileSync(path.join(root, 'build', 'icon.png'), path.join(root, 'resources', 'icon.png'));
  const icons = path.join(root, 'build', 'icons');
  fs.mkdirSync(icons, { recursive: true });
  const entries = [];
  for (const size of [16, 24, 32, 48, 64, 128, 256, 512]) {
    const data = await render(win, size);
    fs.writeFileSync(path.join(icons, `${size}x${size}.png`), data);
    if (size <= 256) entries.push({ size, data });
  }
  fs.writeFileSync(path.join(root, 'build', 'icon.ico'), ico(entries));
  console.log('icons written to build/');
  app.quit();
});
