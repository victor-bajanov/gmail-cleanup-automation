import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fc from 'fast-check';
import {
  stripPrefix,
  diffLabelSegments,
  resolveLabelId,
  type LabelSegment,
} from '../src/lib/label-format.ts';

describe('stripPrefix', () => {
  it('removes matching prefix', () => {
    assert.equal(
      stripPrefix('automanaged/other/airbnb-com', 'automanaged'),
      'other/airbnb-com',
    );
  });

  it('returns unchanged when prefix does not match', () => {
    assert.equal(
      stripPrefix('Promotions/Groupon', 'automanaged'),
      'Promotions/Groupon',
    );
  });

  it('returns unchanged when prefix is empty', () => {
    assert.equal(stripPrefix('automanaged/other', ''), 'automanaged/other');
  });

  it('returns unchanged when label equals prefix (nothing after prefix/)', () => {
    assert.equal(stripPrefix('automanaged', 'automanaged'), 'automanaged');
  });

  it('property: output never starts with prefix/ when label had content after prefix', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.string({ minLength: 1, maxLength: 10 }).filter((s) => !s.includes('/')),
        (suffix, prefix) => {
          const label = `${prefix}/${suffix}`;
          const result = stripPrefix(label, prefix);
          assert.ok(!result.startsWith(`${prefix}/`));
        },
      ),
    );
  });

  it('property: stripping is idempotent', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 0, maxLength: 30 }),
        fc.string({ minLength: 0, maxLength: 10 }),
        (label, prefix) => {
          const once = stripPrefix(label, prefix);
          const twice = stripPrefix(once, prefix);
          assert.equal(once, twice);
        },
      ),
    );
  });
});

describe('diffLabelSegments', () => {
  it('highlights differing first segment', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'marketing/airbnb-com']);
    assert.equal(result[0][0].highlighted, true);
    assert.equal(result[0][1].highlighted, false);
    assert.equal(result[1][0].highlighted, true);
    assert.equal(result[1][1].highlighted, false);
  });

  it('highlights differing last segment', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'other/airbnb-com-au']);
    assert.equal(result[0][0].highlighted, false);
    assert.equal(result[0][1].highlighted, true);
    assert.equal(result[1][0].highlighted, false);
    assert.equal(result[1][1].highlighted, true);
  });

  it('highlights both when both differ', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'marketing/airbnb-com-au']);
    assert.equal(result[0][0].highlighted, true);
    assert.equal(result[0][1].highlighted, true);
    assert.equal(result[1][0].highlighted, true);
    assert.equal(result[1][1].highlighted, true);
  });

  it('single label = nothing highlighted', () => {
    const result = diffLabelSegments(['other/airbnb-com']);
    assert.equal(result[0][0].highlighted, false);
    assert.equal(result[0][1].highlighted, false);
  });

  it('different segment counts = missing segments treated as different', () => {
    const result = diffLabelSegments(['a/b/c', 'a/b']);
    // First two segments identical
    assert.equal(result[0][0].highlighted, false);
    assert.equal(result[0][1].highlighted, false);
    // Third segment only exists in first label -> highlighted
    assert.equal(result[0][2].highlighted, true);
  });

  it('identical labels = nothing highlighted', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'other/airbnb-com']);
    for (const segs of result) {
      for (const seg of segs) {
        assert.equal(seg.highlighted, false);
      }
    }
  });

  it('3+ labels: highlight any segment where not ALL labels agree', () => {
    const result = diffLabelSegments([
      'other/airbnb-com',
      'other/airbnb-com',
      'marketing/airbnb-com',
    ]);
    // First segment differs (other vs marketing)
    assert.equal(result[0][0].highlighted, true);
    assert.equal(result[1][0].highlighted, true);
    assert.equal(result[2][0].highlighted, true);
    // Second segment same across all
    assert.equal(result[0][1].highlighted, false);
    assert.equal(result[1][1].highlighted, false);
    assert.equal(result[2][1].highlighted, false);
  });

  it('property: identical labels produce no highlights', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 15 }),
        fc.integer({ min: 2, max: 5 }),
        (label, count) => {
          const labels = Array.from({ length: count }, () => label);
          const result = diffLabelSegments(labels);
          for (const segs of result) {
            for (const seg of segs) {
              assert.equal(seg.highlighted, false);
            }
          }
        },
      ),
    );
  });

  it('property: single label produces no highlights', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 15 }),
        (label) => {
          const result = diffLabelSegments([label]);
          for (const segs of result) {
            for (const seg of segs) {
              assert.equal(seg.highlighted, false);
            }
          }
        },
      ),
    );
  });

  it('property: segment text rejoined with / equals original label', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.string({ minLength: 1, maxLength: 15 }),
          { minLength: 1, maxLength: 5 },
        ),
        (labels) => {
          const result = diffLabelSegments(labels);
          for (let i = 0; i < labels.length; i++) {
            const rejoined = result[i].map((s) => s.text).join('/');
            assert.equal(rejoined, labels[i]);
          }
        },
      ),
    );
  });
});

describe('resolveLabelId', () => {
  it('resolves and strips', () => {
    assert.equal(
      resolveLabelId('L1', { L1: 'automanaged/other/airbnb-com' }, 'automanaged'),
      'other/airbnb-com',
    );
  });

  it('falls back to raw ID when not in map', () => {
    assert.equal(resolveLabelId('L99', {}, 'automanaged'), 'L99');
  });

  it('property: result is never empty when input id is non-empty', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.dictionary(
          fc.string({ minLength: 1, maxLength: 10 }),
          fc.string({ minLength: 1, maxLength: 30 }),
        ),
        fc.string({ minLength: 0, maxLength: 10 }),
        (id, map, prefix) => {
          const result = resolveLabelId(id, map, prefix);
          assert.ok(result.length > 0);
        },
      ),
    );
  });
});
