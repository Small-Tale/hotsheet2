import { request as httpRequest } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { Readable } from 'node:stream';

export interface RemoteTls {
  ca_pem?: string | null;
  client_certificate_pem?: string | null;
  client_key_pem?: string | null;
}

/** The trusted Node host owns TLS material. Redirects stay visible to the caller so an
 * approved server cannot silently send a session to a different host (HS2-BT4KN6). */
export async function remoteRequest(url: URL, init: RequestInit, tls: RemoteTls = {}): Promise<Response> {
  const body =
    init.body === undefined || init.body === null
      ? undefined
      : Buffer.from(await new Response(init.body).arrayBuffer());
  const headers = new Headers(init.headers);
  if (body) headers.set('content-length', String(body.length));
  const options = {
    method: init.method ?? 'GET',
    headers: Object.fromEntries(headers),
    signal: init.signal ?? undefined,
    ...(tls.ca_pem ? { ca: tls.ca_pem } : {}),
    ...(tls.client_certificate_pem ? { cert: tls.client_certificate_pem } : {}),
    ...(tls.client_key_pem ? { key: tls.client_key_pem } : {}),
  };
  return new Promise<Response>((resolve, reject) => {
    const request = (url.protocol === 'https:' ? httpsRequest : httpRequest)(url, options, (incoming) => {
      const responseHeaders = new Headers();
      for (const [name, value] of Object.entries(incoming.headers)) {
        if (Array.isArray(value)) for (const item of value) responseHeaders.append(name, item);
        else if (value !== undefined) responseHeaders.set(name, value);
      }
      resolve(
        new Response(
          [204, 205, 304].includes(incoming.statusCode ?? 0)
            ? null
            : (Readable.toWeb(incoming) as ReadableStream<Uint8Array>),
          {
            status: incoming.statusCode ?? 502,
            headers: responseHeaders,
          },
        ),
      );
    });
    request.once('error', reject);
    request.end(body);
  });
}
