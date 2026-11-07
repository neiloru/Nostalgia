import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildArguments, discLabel, expandString, launchCandidates, splitArgs, validateEmuConfig } from '@nostalgia/shared';

test('splitArgs honours quotes', () => {
  assert.deepEqual(splitArgs('-nogui -fullscreen "{rom}"'), ['-nogui', '-fullscreen', '{rom}']);
  assert.deepEqual(splitArgs(`-a 'b c' "" d`), ['-a', 'b c', '', 'd']);
});

test('expandString handles placeholders and environment variables', () => {
  const ctx = { vars: { rom: 'C:/g/a b.iso', emulatorDir: '/emu' }, env: { USERPROFILE: 'C:/Users/x', HOME: '/home/x' }, home: '/home/x', os: 'windows' as const };
  assert.equal(expandString('%userprofile%/Documents', ctx), 'C:/Users/x/Documents');
  assert.equal(expandString('{emulatorDir}/memcards', ctx), '/emu/memcards');
  assert.equal(expandString('~/saves', { ...ctx, os: 'linux' }), '/home/x/saves');
  assert.equal(expandString('${HOME}/.config', { ...ctx, os: 'linux' }), '/home/x/.config');
  assert.equal(expandString('{unknown}', ctx), '{unknown}');
});

test('buildArguments keeps paths with spaces as one argument', () => {
  const ctx = { vars: { rom: 'C:/My Games/ff x.iso' }, env: {}, home: '/h', os: 'windows' as const };
  assert.deepEqual(buildArguments({ executable: 'x', arguments: '-nogui -fullscreen "{rom}"' }, ctx), ['-nogui', '-fullscreen', 'C:/My Games/ff x.iso']);
  assert.deepEqual(buildArguments({ executable: 'x' }, ctx), ['C:/My Games/ff x.iso']);
});

test('launchCandidates prefers cue over bin and sorts discs naturally', () => {
  const files = ['igdb/cover.jpg', 'Game (Disc 10).cue', 'Game (Disc 2).bin', 'Game (Disc 2).cue', 'Game (Disc 1).cue', 'Game (Disc 1).bin'];
  assert.deepEqual(launchCandidates(files, ['m3u', 'cue', 'bin']), ['Game (Disc 1).cue', 'Game (Disc 2).cue', 'Game (Disc 10).cue']);
  assert.equal(discLabel('Final Fantasy VIII (USA) (Disc 3).cue'), 'Disc 3');
});

test('validateEmuConfig accepts the pcsx2.info style and rejects broken files', () => {
  const c = validateEmuConfig({ name: 'PCSX2', systems: { windows: { executable: 'pcsx2-qt.exe', saveFolder: '%USERPROFILE%/Documents/PCSX2/memcards' } } });
  assert.deepEqual(c.systems.windows?.saves, ['%USERPROFILE%/Documents/PCSX2/memcards']);
  assert.throws(() => validateEmuConfig({ name: 'x' }));
  assert.throws(() => validateEmuConfig({ systems: { linux: {} } }));
});
