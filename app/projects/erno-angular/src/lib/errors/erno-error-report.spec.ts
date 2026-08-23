import { HttpErrorResponse } from '@angular/common/http';
import { normalizeError, parseStack } from './erno-error-report';

describe('normalizeError', () => {
  it('handles a real Error with a stack', () => {
    const error = new TypeError('x is not a function');
    const report = normalizeError(error);
    expect(report.type).toBe('TypeError');
    expect(report.message).toBe('x is not a function');
    expect(report.level).toBe('error');
  });

  it('handles an HttpErrorResponse', () => {
    const report = normalizeError(
      new HttpErrorResponse({ status: 500, statusText: 'Server Error', url: '/api/decks' }),
    );
    expect(report.type).toBe('HttpErrorResponse');
    expect(report.message).toContain('500');
    expect(report.context?.['status']).toBe(500);
  });

  it('handles a thrown string', () => {
    expect(normalizeError('boom').message).toBe('boom');
  });

  it('handles a thrown object carrying a message', () => {
    const report = normalizeError({ name: 'CustomError', message: 'something odd' });
    expect(report.type).toBe('CustomError');
    expect(report.message).toBe('something odd');
  });

  it('handles a plain object with no message', () => {
    expect(normalizeError({ a: 1 }).message).toBe('{"a":1}');
  });

  it('survives a circular thrown value', () => {
    // JS lets you throw anything, including something JSON cannot serialise.
    const circular: Record<string, unknown> = {};
    circular['self'] = circular;
    expect(() => normalizeError(circular)).not.toThrow();
  });

  it('handles null and undefined', () => {
    expect(normalizeError(null).message).toBe('null');
    expect(normalizeError(undefined).message).toBe('undefined');
  });
});

describe('parseStack', () => {
  it('parses the V8 format', () => {
    const frames = parseStack(
      ['Error: boom', '    at Foo.bar (https://app.test/main.js:12:5)', '    at https://app.test/main.js:40:9'].join('\n'),
    );
    expect(frames).toHaveLength(2);
    expect(frames[0]).toEqual({
      function: 'Foo.bar',
      file: 'https://app.test/main.js',
      line: 12,
      column: 5,
    });
    expect(frames[1].function).toBeUndefined();
    expect(frames[1].line).toBe(40);
  });

  it('parses the Safari/Firefox format', () => {
    const frames = parseStack(['boom', 'foo@https://app.test/main.js:12:5'].join('\n'));
    expect(frames).toHaveLength(1);
    expect(frames[0].function).toBe('foo');
  });

  it('returns nothing for an absent or unparseable stack', () => {
    expect(parseStack(undefined)).toEqual([]);
    expect(parseStack('no frames here')).toEqual([]);
  });

  it('is bounded', () => {
    const stack = ['Error', ...Array.from({ length: 500 }, (_, i) => `    at f${i} (a.js:${i}:1)`)].join('\n');
    expect(parseStack(stack).length).toBeLessThanOrEqual(50);
  });
});
