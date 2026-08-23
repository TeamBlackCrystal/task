import arkenv from 'arkenv';
import dotenv from 'dotenv';
import { Elysia } from 'elysia';

// API_BASE の検証と既定値は ../api-base (単一ソース) に寄せてある。
import { API_BASE } from '../api-base';

// The production SSR entry runs independently of Vite, so load runtime values
// before arkenv validates process.env. Existing process variables still win.
dotenv.config({ quiet: true });

const env = arkenv({
  UPLOAD_MAX_SIZE_MB: 'number > 0 = 100',
});

/** Align with backend UPLOAD_MAX_SIZE_MB default (100). */
export const MAX_PROXY_BODY_BYTES = env.UPLOAD_MAX_SIZE_MB * 1024 * 1024;

const HOP_BY_HOP = new Set([
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailers',
  'transfer-encoding',
  'upgrade',
  'host',
]);

class BodyTooLargeError extends Error {
  constructor() {
    super('Payload Too Large');
    this.name = 'BodyTooLargeError';
  }
}

function buildBackendUrl(request: Request): string {
  const url = new URL(request.url);
  const backendPath = url.pathname.replace(/^\/api/, '') + url.search;
  return `${API_BASE}${backendPath}`;
}

function copyHeaders(source: Headers, skipHopByHop = true): Headers {
  const headers = new Headers();
  source.forEach((value, key) => {
    const lowerKey = key.toLowerCase();
    if (skipHopByHop && HOP_BY_HOP.has(lowerKey)) return;
    if (lowerKey === 'set-cookie') return;
    headers.set(key, value);
  });
  for (const cookie of source.getSetCookie()) {
    headers.append('Set-Cookie', cookie);
  }
  return headers;
}

function rejectIfContentLengthTooLarge(request: Request): Response | null {
  const contentLength = request.headers.get('content-length');
  if (!contentLength) return null;

  const length = Number(contentLength);
  if (!Number.isFinite(length) || length < 0) return null;
  if (length > MAX_PROXY_BODY_BYTES) {
    return new Response('Payload Too Large', { status: 413 });
  }
  return null;
}

export type LimitedReadableStream = {
  stream: ReadableStream<Uint8Array>;
  readonly exceeded: boolean;
};

export function limitReadableStream(
  body: ReadableStream<Uint8Array>,
  maxBytes: number,
): LimitedReadableStream {
  let consumed = 0;
  let exceeded = false;
  const reader = body.getReader();

  const stream = new ReadableStream({
    async pull(controller) {
      const { done, value } = await reader.read();
      if (done) {
        controller.close();
        return;
      }

      consumed += value.byteLength;
      if (consumed > maxBytes) {
        exceeded = true;
        await reader.cancel();
        controller.error(new BodyTooLargeError());
        return;
      }

      controller.enqueue(value);
    },
    cancel(reason) {
      return reader.cancel(reason);
    },
  });

  return {
    stream,
    get exceeded() {
      return exceeded;
    },
  };
}

async function proxyToBackend(request: Request): Promise<Response> {
  const rejected = rejectIfContentLengthTooLarge(request);
  if (rejected) return rejected;

  const backendUrl = buildBackendUrl(request);
  const hasBody = request.method !== 'GET' && request.method !== 'HEAD';

  // parse: 'none' keeps Elysia from consuming the body, so the proxy can stream it
  // through with a byte limit instead of buffering the full payload.
  const limited =
    hasBody && request.body ? limitReadableStream(request.body, MAX_PROXY_BODY_BYTES) : undefined;
  const body = limited?.stream;

  try {
    const backendResponse = await fetch(backendUrl, {
      method: request.method,
      headers: copyHeaders(request.headers),
      body,
      // @ts-expect-error Node/Bun fetch requires duplex when streaming a request body
      duplex: hasBody ? 'half' : undefined,
    });

    return new Response(backendResponse.body, {
      status: backendResponse.status,
      statusText: backendResponse.statusText,
      headers: copyHeaders(backendResponse.headers),
    });
  } catch (error) {
    const cause = error instanceof Error ? error.cause : undefined;
    if (
      limited?.exceeded ||
      error instanceof BodyTooLargeError ||
      cause instanceof BodyTooLargeError
    ) {
      return new Response('Payload Too Large', { status: 413 });
    }
    throw error;
  }
}

/**
 * Dev/prod SSR: forward /api/v1/* to the Rust backend with /api stripped.
 * Local SSR routes (/internal/*) stay on Elysia.
 */
export const apiProxyPlugin = new Elysia({ name: 'api-proxy' }).all(
  '/api/v1/*',
  ({ request }) => proxyToBackend(request),
  { parse: 'none' },
);
