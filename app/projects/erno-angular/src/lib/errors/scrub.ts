/**
 * Secret redaction, mirroring `api/src/error_reporting/scrub.rs`.
 *
 * Docs: docs/src/content/docs/app/error-reporting.md
 *
 * Both implementations are necessary and neither is redundant. This one is the
 * only thing that can stop a secret *leaving the device* — and with the
 * collector on separate infrastructure, that now means leaving for a different
 * provider entirely, past whatever proxies and CDN logs sit in between. The
 * server-side pass is the only thing that can stop it being *stored*.
 *
 * The two rule sets are kept in step by a table in the docs and matching test
 * fixtures on each side.
 */

/** What a redacted value becomes. Visible, so triage sees a marker not a gap. */
export const REDACTED = '[redacted]';
/** How deep scrubbing descends into nested structures. */
const MAX_DEPTH = 5;
/** What an over-deep structure becomes. */
export const TRUNCATED = '[truncated: too deep]';

const SENSITIVE_TOKENS = new Set([
  'auth',
  'authorization',
  'token',
  'jwt',
  'password',
  'passwd',
  'pwd',
  'secret',
  'apikey',
  'key',
  'credential',
  'credentials',
  'cookie',
  'cookies',
  'session',
  'csrf',
  'xsrf',
  'signature',
  'card',
  'cvv',
  'cvc',
  'ssn',
]);

/**
 * Split a key into lowercase word tokens on non-alphanumerics and camelCase
 * boundaries, so `author` is not mistaken for `auth`.
 */
function keyTokens(key: string): string[] {
  const tokens: string[] = [];
  let current = '';
  for (let i = 0; i < key.length; i++) {
    const ch = key[i];
    if (!/[a-zA-Z0-9]/.test(ch)) {
      if (current) {
        tokens.push(current);
        current = '';
      }
      continue;
    }
    const previous = key[i - 1];
    if (/[A-Z]/.test(ch) && i > 0 && /[a-z0-9]/.test(previous) && current) {
      tokens.push(current);
      current = '';
    }
    current += ch.toLowerCase();
  }
  if (current) {
    tokens.push(current);
  }
  return tokens;
}

/** Whether a key name marks its value as sensitive. */
export function isSensitiveKey(key: string): boolean {
  const tokens = keyTokens(key);
  if (tokens.some((t) => SENSITIVE_TOKENS.has(t))) {
    return true;
  }
  const joined = tokens.join('');
  return ['token', 'password', 'secret', 'apikey', 'cookie'].some((n) => joined.includes(n));
}

const JWT = /eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*/g;
const SCHEME = /\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{6,}/gi;
const CARD = /\b(?:\d[ -]?){13,19}\b/g;
const OPAQUE = /\b[A-Za-z0-9_-]{32,}\b/g;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Redact secret-shaped substrings in free text. */
export function scrubText(input: string): string {
  return input
    .replace(JWT, REDACTED)
    .replace(SCHEME, REDACTED)
    .replace(CARD, REDACTED)
    .replace(OPAQUE, (match) =>
      // A UUID is long enough to look opaque but is an identifier, not a
      // credential — and it is often the one detail that makes an error
      // traceable to a record.
      UUID.test(match) ? match : REDACTED,
    );
}

/**
 * Scrub a URL.
 *
 * The fragment is dropped wholesale: Erno's own share links carry their secret
 * there, so a shared-link URL in an error report would otherwise hand out
 * access to the shared resource.
 */
export function scrubUrl(url: string): string {
  const withoutFragment = url.split('#')[0];
  const [path, query] = withoutFragment.split('?');
  if (query === undefined) {
    return scrubText(path);
  }

  const scrubbed = query
    .split('&')
    .map((pair) => {
      const index = pair.indexOf('=');
      if (index === -1) {
        return scrubText(pair);
      }
      const key = pair.slice(0, index);
      const value = pair.slice(index + 1);
      return isSensitiveKey(key) ? `${key}=${REDACTED}` : `${key}=${scrubText(value)}`;
    })
    .join('&');

  return `${scrubText(path)}?${scrubbed}`;
}

/** Recursively scrub an arbitrary value, returning a safe copy. */
export function scrubValue(value: unknown, depth = 0): unknown {
  if (typeof value === 'string') {
    return scrubText(value);
  }
  if (Array.isArray(value)) {
    if (depth >= MAX_DEPTH) {
      return TRUNCATED;
    }
    return value.map((item) => scrubValue(item, depth + 1));
  }
  if (value && typeof value === 'object') {
    if (depth >= MAX_DEPTH) {
      return TRUNCATED;
    }
    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      if (isSensitiveKey(key)) {
        out[key] = REDACTED;
        continue;
      }
      if ((key === 'url' || key === 'referrer') && typeof child === 'string') {
        out[key] = scrubUrl(child);
        continue;
      }
      out[key] = scrubValue(child, depth + 1);
    }
    return out;
  }
  return value;
}

/** Scrub a context object, always returning an object. */
export function scrubContext(context: Record<string, unknown> | undefined): Record<string, unknown> {
  if (!context) {
    return {};
  }
  return scrubValue(context) as Record<string, unknown>;
}
