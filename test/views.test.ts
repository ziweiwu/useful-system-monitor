import { describe, expect, it } from 'vitest';
import { stepView, VIEW_KEYS, VIEW_LABELS, VIEW_ORDER, viewKey } from '../src/core/views.js';

describe('I-27: view navigation', () => {
  it('steps forward and backward through the strip', () => {
    expect(stepView('overview', 1)).toBe('cpu');
    expect(stepView('cpu', 1)).toBe('memory');
    expect(stepView('memory', -1)).toBe('cpu');
  });

  it('wraps at both ends, so neither arrow is ever a dead key', () => {
    expect(stepView('disk', 1)).toBe('overview');
    expect(stepView('overview', -1)).toBe('disk');
  });

  it('returns to the same view after a full lap in either direction', () => {
    for (const v of VIEW_ORDER) {
      let f = v;
      let b = v;
      for (let i = 0; i < VIEW_ORDER.length; i++) {
        f = stepView(f, 1);
        b = stepView(b, -1);
      }
      expect(f).toBe(v);
      expect(b).toBe(v);
    }
  });

  it('keeps the number keys and the strip order in sync', () => {
    // The tab strip prints viewKey(); the keymap reads VIEW_KEYS. If these
    // disagreed, the label on screen would open a different screen.
    for (const v of VIEW_ORDER) {
      expect(VIEW_KEYS[viewKey(v)]).toBe(v);
    }
    expect(Object.keys(VIEW_KEYS)).toEqual(['1', '2', '3', '4', '5']);
  });

  it('labels every view', () => {
    for (const v of VIEW_ORDER) expect(VIEW_LABELS[v]).toMatch(/^[A-Z]+$/);
  });
});
