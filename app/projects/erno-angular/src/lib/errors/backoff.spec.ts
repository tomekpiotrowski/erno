import { JITTER_FRACTION, MAX_DELAY_MS, baseDelayMs, nextDelayMs } from './backoff';

describe('backoff', () => {
  it('doubles then flattens at the ceiling', () => {
    expect(baseDelayMs(1)).toBe(1_000);
    expect(baseDelayMs(2)).toBe(2_000);
    expect(baseDelayMs(3)).toBe(4_000);
    expect(baseDelayMs(6)).toBe(MAX_DELAY_MS);
    expect(baseDelayMs(50)).toBe(MAX_DELAY_MS);
  });

  it('does not wait before the first attempt', () => {
    expect(baseDelayMs(0)).toBe(0);
    expect(nextDelayMs(0)).toBe(0);
  });

  it('keeps jitter inside ±20%', () => {
    const base = baseDelayMs(3);
    const spread = base * JITTER_FRACTION;
    for (const random of [() => 0, () => 0.5, () => 0.999]) {
      const delay = nextDelayMs(3, random);
      expect(delay).toBeGreaterThanOrEqual(Math.round(base - spread));
      expect(delay).toBeLessThanOrEqual(Math.round(base + spread));
    }
  });
});
