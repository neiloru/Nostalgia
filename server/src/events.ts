import type { Response } from 'express';
import type { ServerEvent } from '@nostalgia/shared';

interface Client {
  res: Response;
  userId: number;
  device?: string;
}

/** Server-Sent-Events hub used to push new cloud saves / library changes to open clients. */
export class EventHub {
  private clients = new Set<Client>();

  constructor() {
    // Keep connections alive through proxies.
    setInterval(() => {
      for (const c of this.clients) c.res.write(': ping\n\n');
    }, 25_000).unref();
  }

  add(res: Response, userId: number, device?: string) {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Accel-Buffering': 'no',
    });
    const client: Client = { res, userId, device };
    this.clients.add(client);
    this.send(client, { type: 'hello', serverTime: new Date().toISOString() });
    res.on('close', () => this.clients.delete(client));
  }

  private send(c: Client, ev: ServerEvent) {
    c.res.write(`event: ${ev.type}\ndata: ${JSON.stringify(ev)}\n\n`);
  }

  toUser(userId: number, ev: ServerEvent, exceptDevice?: string) {
    for (const c of this.clients) if (c.userId === userId && (!exceptDevice || c.device !== exceptDevice)) this.send(c, ev);
  }

  toAll(ev: ServerEvent) {
    for (const c of this.clients) this.send(c, ev);
  }

  get count() {
    return this.clients.size;
  }

  onlineUserIds(): number[] {
    return [...new Set([...this.clients].map((c) => c.userId))];
  }
}
