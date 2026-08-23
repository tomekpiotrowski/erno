import { REDACTED, TRUNCATED, isSensitiveKey, scrubContext, scrubText, scrubUrl } from './scrub';

// Mirrors api/src/error_reporting/scrub.rs. Both implementations are needed:
// this one stops secrets leaving the device, that one stops them being stored.
describe('scrub', () => {
  it('recognises sensitive key names', () => {
    for (const key of [
      'authorization',
      'Authorization',
      'auth',
      'access_token',
      'accessToken',
      'refresh_token',
      'password',
      'userPassword',
      'api_key',
      'apiKey',
      'Cookie',
      'set-cookie',
      'session',
      'csrf_token',
      'private_key',
      'creditCard',
      'cvv',
      'ssn',
    ]) {
      expect(isSensitiveKey(key)).toBe(true);
    }
  });

  it('keeps innocent keys that merely contain a needle', () => {
    // The reason keys are tokenised rather than substring-matched.
    for (const key of ['author', 'authored_at', 'standard', 'discard', 'cardinality']) {
      expect(isSensitiveKey(key)).toBe(false);
    }
  });

  it('redacts JWTs, auth schemes, cards and long opaque runs', () => {
    expect(scrubText('got eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abcdEFGH1234')).toContain(REDACTED);
    expect(scrubText('Bearer abc123def456')).toBe(REDACTED);
    expect(scrubText('card 4111 1111 1111 1111')).toContain(REDACTED);
    expect(scrubText('a'.repeat(40))).toBe(REDACTED);
  });

  it('leaves UUIDs alone', () => {
    // Identifiers, not credentials — and often the one detail that makes an
    // error traceable back to a record.
    const id = '550e8400-e29b-41d4-a716-446655440000';
    expect(scrubText(`sync failed for ${id}`)).toBe(`sync failed for ${id}`);
  });

  it('drops the URL fragment entirely', () => {
    // Erno share links carry their secret in the fragment.
    const out = scrubUrl('https://app.test/shared#tok=supersecretvalue');
    expect(out).not.toContain('supersecretvalue');
    expect(out).not.toContain('#');
  });

  it('redacts sensitive query parameters and keeps the rest', () => {
    const out = scrubUrl('https://app.test/x?page=2&access_token=abc123&sort=name');
    expect(out).toContain('page=2');
    expect(out).toContain('sort=name');
    expect(out).toContain(`access_token=${REDACTED}`);
  });

  it('scrubs nested context objects', () => {
    const out = scrubContext({
      route: '/decks/:id',
      headers: { authorization: 'Bearer abc123def456', accept: 'application/json' },
      extra: { nested: { password: 'hunter2', keep: 'fine' } },
    }) as Record<string, any>;

    expect(out['headers'].authorization).toBe(REDACTED);
    expect(out['headers'].accept).toBe('application/json');
    expect(out['extra'].nested.password).toBe(REDACTED);
    expect(out['extra'].nested.keep).toBe('fine');
    expect(out['route']).toBe('/decks/:id');
  });

  it('stops descending past the depth limit', () => {
    let value: any = { password: 'leaf' };
    for (let i = 0; i < 10; i++) {
      value = { next: value };
    }
    const rendered = JSON.stringify(scrubContext(value));
    expect(rendered).not.toContain('leaf');
    expect(rendered).toContain(TRUNCATED);
  });
});
