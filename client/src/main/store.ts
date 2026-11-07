import fs from 'node:fs';
import path from 'node:path';

/** Tiny JSON persistence with atomic writes. */
export class JsonStore<T extends object> {
  private value: T;
  private timer: NodeJS.Timeout | null = null;

  constructor(private file: string, defaults: NoInfer<T>) {
    this.value = defaults;
    try {
      if (fs.existsSync(file)) this.value = { ...defaults, ...(JSON.parse(fs.readFileSync(file, 'utf8')) as T) };
    } catch (e) {
      console.error(`Could not read ${file}, keeping a copy and starting fresh`, e);
      try {
        fs.copyFileSync(file, `${file}.corrupt-${Date.now()}`);
      } catch {
        /* ignore */
      }
    }
  }

  get data(): T {
    return this.value;
  }

  set(update: Partial<T> | ((v: T) => void)) {
    if (typeof update === 'function') update(this.value);
    else this.value = { ...this.value, ...update };
    this.saveSoon();
  }

  saveSoon() {
    if (this.timer) return;
    this.timer = setTimeout(() => this.saveNow(), 250);
  }

  saveNow() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    const tmp = `${this.file}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(this.value, null, 2));
    fs.renameSync(tmp, this.file);
  }
}
