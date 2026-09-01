// @vitest-environment node
import { Elysia } from 'elysia';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MAX_PROXY_BODY_BYTES, apiProxyPlugin, limitReadableStream } from '../api-proxy';

const app = new Elysia().use(apiProxyPlugin);
const DEFAULT_MAX_PROXY_BODY_BYTES = 100 * 1024 * 1024;

async function proxyRequest(request: Request): Promise<Response> {
  return app.handle(request);
}

async function readStreamBody(body: ReadableStream<Uint8Array> | null): Promise<Uint8Array> {
  if (!body) return new Uint8Array();

  const reader = body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    total += value.byteLength;
  }

  const merged = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  }

  return merged;
}

describe('limitReadableStream', () => {
  it('cancels the source reader when streamed bytes exceed the configured max', async () => {
    let sourceCancelCalled = false;
    const source = new ReadableStream<Uint8Array>({
      pull(controller) {
        controller.enqueue(new Uint8Array(64));
      },
      cancel() {
        sourceCancelCalled = true;
      },
    });

    const limited = limitReadableStream(source, 96);
    const reader = limited.stream.getReader();

    await expect(reader.read()).resolves.toEqual({
      done: false,
      value: new Uint8Array(64),
    });
    await expect(reader.read()).rejects.toThrow('Payload Too Large');
    expect(sourceCancelCalled).toBe(true);
  });
});

describe('MAX_PROXY_BODY_BYTES', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it('parses a positive UPLOAD_MAX_SIZE_MB value', async () => {
    vi.stubEnv('UPLOAD_MAX_SIZE_MB', '2.5');
    vi.resetModules();

    const proxy = await import('../api-proxy');

    expect(proxy.MAX_PROXY_BODY_BYTES).toBe(2.5 * 1024 * 1024);
  });

  it('defaults to 100 MiB when UPLOAD_MAX_SIZE_MB is unset', async () => {
    vi.stubEnv('UPLOAD_MAX_SIZE_MB', undefined);
    vi.resetModules();

    const proxy = await import('../api-proxy');

    expect(proxy.MAX_PROXY_BODY_BYTES).toBe(DEFAULT_MAX_PROXY_BODY_BYTES);
  });

  it.each(['', 'abc', 'NaN', '0', '-1'])(
    'fails fast when UPLOAD_MAX_SIZE_MB is %j',
    async (value) => {
      vi.stubEnv('UPLOAD_MAX_SIZE_MB', value);
      vi.resetModules();

      await expect(import('../api-proxy')).rejects.toThrow(/UPLOAD_MAX_SIZE_MB/);
    },
  );
});

describe('API_BASE', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it.each(['', 'not-a-url'])('fails fast when API_BASE is %j', async (value) => {
    vi.stubEnv('API_BASE', value);
    vi.resetModules();

    await expect(import('../api-proxy')).rejects.toThrow(/API_BASE/);
  });

  it('defaults to localhost when API_BASE is unset', async () => {
    vi.stubEnv('API_BASE', undefined);
    vi.resetModules();
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response('ok'));
    vi.stubGlobal('fetch', fetchMock);

    const { apiProxyPlugin: defaultProxyPlugin } = await import('../api-proxy');
    const defaultApp = new Elysia().use(defaultProxyPlugin);
    await defaultApp.handle(new Request('http://localhost/api/v1/items'));

    expect(fetchMock).toHaveBeenCalledWith('http://localhost:3400/v1/items', expect.any(Object));
    vi.unstubAllGlobals();
  });
});

/**
 * Uses the real fetch against a stub backend, because a mocked fetch cannot
 * reproduce redirect following — the behaviour that broke GitHub integration in
 * production. The install callback passes the repository-select token in the
 * Location fragment; if the proxy resolves the redirect itself, the browser gets
 * the target page as 200 and the token never arrives.
 */
describe('backend redirects (real fetch)', () => {
  let server: Server;
  let landed = false;

  beforeEach(async () => {
    landed = false;
    server = createServer((req, res) => {
      if (req.url?.startsWith('/v1/github/callback')) {
        const port = (server.address() as AddressInfo).port;
        res.writeHead(307, {
          location: `http://127.0.0.1:${port}/landed?section=integrations#github_select=token-1`,
        });
        res.end();
        return;
      }
      // Only reachable when the redirect was followed server-side.
      landed = true;
      res.writeHead(200, { 'content-type': 'text/html' });
      res.end('<html>landed</html>');
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));

    const { port } = server.address() as AddressInfo;
    vi.stubEnv('API_BASE', `http://127.0.0.1:${port}`);
    vi.resetModules();
  });

  afterEach(async () => {
    vi.unstubAllEnvs();
    vi.resetModules();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  });

  it('hands the 307 and its Location to the browser without fetching the target', async () => {
    const { apiProxyPlugin: plugin } = await import('../api-proxy');
    const proxyApp = new Elysia().use(plugin);

    const response = await proxyApp.handle(
      new Request('http://localhost/api/v1/github/callback?installation_id=1'),
    );

    expect(response.status).toBe(307);
    expect(response.headers.get('location')).toContain('#github_select=token-1');
    expect(landed).toBe(false);
  });
});

describe('.env loading', () => {
  const originalCwd = process.cwd();
  let temporaryDirectory: string | undefined;

  afterEach(async () => {
    process.chdir(originalCwd);
    delete process.env.API_BASE;
    delete process.env.UPLOAD_MAX_SIZE_MB;
    vi.unstubAllGlobals();
    vi.resetModules();
    if (temporaryDirectory) await rm(temporaryDirectory, { recursive: true });
    temporaryDirectory = undefined;
  });

  it('loads API_BASE and UPLOAD_MAX_SIZE_MB from the runtime .env before validation', async () => {
    temporaryDirectory = await mkdtemp(path.join(tmpdir(), 'api-proxy-env-'));
    await writeFile(
      path.join(temporaryDirectory, '.env'),
      'API_BASE=https://api.example.com\nUPLOAD_MAX_SIZE_MB=2.5\n',
    );
    delete process.env.API_BASE;
    delete process.env.UPLOAD_MAX_SIZE_MB;
    process.chdir(temporaryDirectory);
    vi.resetModules();

    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response('ok'));
    vi.stubGlobal('fetch', fetchMock);
    const proxy = await import('../api-proxy');
    const envApp = new Elysia().use(proxy.apiProxyPlugin);
    await envApp.handle(new Request('http://localhost/api/v1/items'));

    expect(proxy.MAX_PROXY_BODY_BYTES).toBe(2.5 * 1024 * 1024);
    expect(fetchMock).toHaveBeenCalledWith('https://api.example.com/v1/items', expect.any(Object));
  });
});

describe('apiProxyPlugin', () => {
  const fetchMock = vi.fn<typeof fetch>();

  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('returns 413 before backend fetch when Content-Length exceeds MAX_PROXY_BODY_BYTES', async () => {
    const response = await proxyRequest(
      new Request('http://localhost/api/v1/upload', {
        method: 'POST',
        headers: {
          'content-type': 'application/octet-stream',
          'content-length': String(MAX_PROXY_BODY_BYTES + 1),
        },
        body: 'too-large',
      }),
    );

    expect(response.status).toBe(413);
    expect(await response.text()).toBe('Payload Too Large');
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('returns 413 when chunked body exceeds MAX_PROXY_BODY_BYTES without Content-Length', async () => {
    const totalBytes = MAX_PROXY_BODY_BYTES + 1;
    const chunkSize = 64 * 1024;
    let sent = 0;

    const oversizedBody = new ReadableStream({
      pull(controller) {
        if (sent >= totalBytes) {
          controller.close();
          return;
        }

        const size = Math.min(chunkSize, totalBytes - sent);
        controller.enqueue(new Uint8Array(size));
        sent += size;
      },
    });

    fetchMock.mockImplementation(async (_url, init) => {
      try {
        await readStreamBody(init?.body as ReadableStream<Uint8Array> | null);
      } catch (cause) {
        // undici wraps stream errors as TypeError('fetch failed', { cause })
        throw new TypeError('fetch failed', { cause });
      }
      return new Response('ok', { status: 200 });
    });

    const response = await proxyRequest(
      new Request('http://localhost/api/v1/upload', {
        method: 'POST',
        headers: { 'content-type': 'application/octet-stream' },
        body: oversizedBody,
        // @ts-expect-error Node fetch requires duplex when streaming a request body
        duplex: 'half',
      }),
    );

    expect(response.status).toBe(413);
    expect(await response.text()).toBe('Payload Too Large');
  });

  /**
   * The GitHub App install callback hands the repository-select token to the
   * browser in the Location fragment (`#github_select=...`). With fetch's
   * default `redirect: 'follow'` the proxy resolves the redirect server-side and
   * returns the target page as 200, so the browser never sees Location and the
   * token is lost. That took down repository selection in production.
   */
  it('returns backend redirects to the browser instead of following them', async () => {
    const location =
      'https://task.example.com/koyori/projects/TASK/settings?section=integrations#github_select=token-1';
    fetchMock.mockResolvedValue(new Response(null, { status: 307, headers: { location } }));

    const response = await proxyRequest(new Request('http://localhost/api/v1/github/callback'));

    expect(response.status).toBe(307);
    expect(response.headers.get('location')).toBe(location);
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it('asks the backend fetch not to follow redirects', async () => {
    fetchMock.mockResolvedValue(new Response('ok'));

    await proxyRequest(new Request('http://localhost/api/v1/github/callback'));

    expect(fetchMock).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({ redirect: 'manual' }),
    );
  });

  it('forwards an under-limit streamed body to the backend unchanged', async () => {
    const payload = new TextEncoder().encode('hello-proxy-body');
    let forwardedBody: Uint8Array | null = null;

    fetchMock.mockImplementation(async (_url, init) => {
      forwardedBody = await readStreamBody(init?.body as ReadableStream<Uint8Array> | null);
      return new Response('proxied', { status: 201 });
    });

    const response = await proxyRequest(
      new Request('http://localhost/api/v1/items', {
        method: 'POST',
        headers: {
          'content-type': 'text/plain',
          'content-length': String(payload.byteLength),
        },
        body: payload,
      }),
    );

    expect(response.status).toBe(201);
    expect(await response.text()).toBe('proxied');
    expect(forwardedBody).toEqual(payload);
    expect(fetchMock).toHaveBeenCalledOnce();

    const [backendUrl, init] = fetchMock.mock.calls[0]!;
    expect(backendUrl).toBe('http://localhost:3400/v1/items');
    expect(init?.method).toBe('POST');
  });
});
