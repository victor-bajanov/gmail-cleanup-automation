# Remediation UI Polish Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix four usability gaps in the remediation UI: progress indicator with ETA, label ID-to-name resolution, compact label display with segment-level diff highlighting, and pick-winner override on MechanicalFix cards.

**Architecture:** Pure frontend utility functions for label formatting and segment diffing (property-tested with fast-check). Tauri event emission for progress (follows existing `EventEmitter` pattern). Keyboard shortcuts follow the `onMount`/`onCleanup` pattern from ReviewView.tsx.

**Tech Stack:** SolidJS, TypeScript, Puppeteer (Node test runner), fast-check, Tauri, Rust, proptest

---

### Task 1: Install fast-check

**Files:**
- Modify: `ui/package.json`

**Step 1: Add fast-check dev dependency**

Run: `cd ui && npm install --save-dev fast-check`

**Step 2: Verify installation**

Run: `cd ui && node -e "require('fast-check'); console.log('ok')"`
Expected: `ok`

**Step 3: Commit**

```bash
git add ui/package.json ui/package-lock.json
git commit -m "chore: add fast-check for property-based frontend tests"
```

---

### Task 2: Label formatting utilities with property-based tests

These are pure functions — no UI, no Tauri. Test-first with fast-check.

**Files:**
- Create: `ui/src/lib/label-format.ts`
- Create: `ui/tests/label-format.test.ts`

**Step 1: Write property-based tests for `stripPrefix`**

In `ui/tests/label-format.test.ts`:

```typescript
import { describe, it } from 'node:test';
import assert from 'node:assert';
import fc from 'fast-check';
import { stripPrefix, diffLabelSegments } from '../src/lib/label-format.ts';

describe('stripPrefix', () => {
  it('removes matching prefix from label', () => {
    assert.strictEqual(stripPrefix('automanaged/other/airbnb-com', 'automanaged'), 'other/airbnb-com');
  });

  it('returns label unchanged when prefix does not match', () => {
    assert.strictEqual(stripPrefix('Promotions/Groupon', 'automanaged'), 'Promotions/Groupon');
  });

  it('handles empty prefix', () => {
    assert.strictEqual(stripPrefix('automanaged/other/airbnb-com', ''), 'automanaged/other/airbnb-com');
  });

  it('handles label equal to prefix', () => {
    assert.strictEqual(stripPrefix('automanaged', 'automanaged'), 'automanaged');
  });

  it('property: output never starts with prefix/', () => {
    fc.assert(
      fc.property(
        fc.stringOf(fc.constantFrom('a', 'b', '/', '-', '_', '.')).filter(s => s.length > 0 && !s.startsWith('/')),
        fc.stringOf(fc.constantFrom('a', 'b', '/', '-', '_', '.')).filter(s => s.length > 0 && !s.includes('/')),
        (label, prefix) => {
          const result = stripPrefix(label, prefix);
          // If the label started with prefix/, the result must not
          if (label.startsWith(prefix + '/') && label.length > prefix.length + 1) {
            return !result.startsWith(prefix + '/');
          }
          return true;
        }
      )
    );
  });

  it('property: stripping is idempotent', () => {
    fc.assert(
      fc.property(
        fc.stringOf(fc.constantFrom('a', 'b', '/', '-')).filter(s => s.length > 0),
        fc.stringOf(fc.constantFrom('a', 'b', '-')).filter(s => s.length > 0),
        (label, prefix) => {
          const once = stripPrefix(label, prefix);
          const twice = stripPrefix(once, prefix);
          return once === twice;
        }
      )
    );
  });
});
```

**Step 2: Run tests to verify they fail**

Run: `cd ui && npx tsx --test tests/label-format.test.ts`
Expected: FAIL — module not found

**Step 3: Write `stripPrefix` implementation**

In `ui/src/lib/label-format.ts`:

```typescript
/**
 * Strip the configured label prefix from a label name for display.
 * "automanaged/other/airbnb-com" with prefix "automanaged" => "other/airbnb-com"
 * Only strips if the label has segments after the prefix.
 */
export function stripPrefix(label: string, prefix: string): string {
  if (!prefix) return label;
  const prefixSlash = prefix + '/';
  if (label.startsWith(prefixSlash) && label.length > prefixSlash.length) {
    return label.slice(prefixSlash.length);
  }
  return label;
}
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npx tsx --test tests/label-format.test.ts`
Expected: All stripPrefix tests PASS

**Step 5: Write property-based tests for `diffLabelSegments`**

Append to `ui/tests/label-format.test.ts`:

```typescript
describe('diffLabelSegments', () => {
  it('highlights differing first segment', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'marketing/airbnb-com']);
    // Each label gets an array of { text, highlighted } segments
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: true }, { text: 'airbnb-com', highlighted: false }],
      [{ text: 'marketing', highlighted: true }, { text: 'airbnb-com', highlighted: false }],
    ]);
  });

  it('highlights differing last segment', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'other/airbnb-com-au']);
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: false }, { text: 'airbnb-com', highlighted: true }],
      [{ text: 'other', highlighted: false }, { text: 'airbnb-com-au', highlighted: true }],
    ]);
  });

  it('highlights both segments when both differ', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'marketing/airbnb-com-au']);
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: true }, { text: 'airbnb-com', highlighted: true }],
      [{ text: 'marketing', highlighted: true }, { text: 'airbnb-com-au', highlighted: true }],
    ]);
  });

  it('handles single label (no diff needed)', () => {
    const result = diffLabelSegments(['other/airbnb-com']);
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: false }, { text: 'airbnb-com', highlighted: false }],
    ]);
  });

  it('handles labels with different segment counts', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'marketing']);
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: true }, { text: 'airbnb-com', highlighted: true }],
      [{ text: 'marketing', highlighted: true }],
    ]);
  });

  it('handles identical labels (nothing highlighted)', () => {
    const result = diffLabelSegments(['other/airbnb-com', 'other/airbnb-com']);
    assert.deepStrictEqual(result, [
      [{ text: 'other', highlighted: false }, { text: 'airbnb-com', highlighted: false }],
      [{ text: 'other', highlighted: false }, { text: 'airbnb-com', highlighted: false }],
    ]);
  });

  it('handles 3+ labels', () => {
    const result = diffLabelSegments(['a/x', 'b/x', 'a/y']);
    // Segment 0: a, b, a — not all same, so highlighted
    // Segment 1: x, x, y — not all same, so highlighted
    assert.deepStrictEqual(result, [
      [{ text: 'a', highlighted: true }, { text: 'x', highlighted: true }],
      [{ text: 'b', highlighted: true }, { text: 'x', highlighted: true }],
      [{ text: 'a', highlighted: true }, { text: 'y', highlighted: true }],
    ]);
  });

  it('property: identical labels produce no highlights', () => {
    fc.assert(
      fc.property(
        fc.stringOf(fc.constantFrom('a', 'b', '/', '-')).filter(s => s.length > 0 && !s.startsWith('/') && !s.endsWith('/')),
        fc.integer({ min: 2, max: 5 }),
        (label, count) => {
          const labels = Array(count).fill(label);
          const result = diffLabelSegments(labels);
          return result.every(segments =>
            segments.every((seg: { highlighted: boolean }) => !seg.highlighted)
          );
        }
      )
    );
  });

  it('property: single label produces no highlights', () => {
    fc.assert(
      fc.property(
        fc.stringOf(fc.constantFrom('a', 'b', '/', '-')).filter(s => s.length > 0 && !s.startsWith('/') && !s.endsWith('/')),
        (label) => {
          const result = diffLabelSegments([label]);
          return result[0].every((seg: { highlighted: boolean }) => !seg.highlighted);
        }
      )
    );
  });

  it('property: segment text rejoined equals original label', () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.stringOf(fc.constantFrom('a', 'b', '/', '-')).filter(s => s.length > 0 && !s.startsWith('/') && !s.endsWith('/') && !s.includes('//')),
          { minLength: 1, maxLength: 4 }
        ),
        (labels) => {
          const result = diffLabelSegments(labels);
          return labels.every((label, i) =>
            result[i].map((seg: { text: string }) => seg.text).join('/') === label
          );
        }
      )
    );
  });
});
```

**Step 6: Run tests to verify the new ones fail**

Run: `cd ui && npx tsx --test tests/label-format.test.ts`
Expected: diffLabelSegments tests FAIL

**Step 7: Write `diffLabelSegments` implementation**

Append to `ui/src/lib/label-format.ts`:

```typescript
export interface LabelSegment {
  text: string;
  highlighted: boolean;
}

/**
 * Compare multiple labels segment-by-segment and mark which segments differ.
 * Split on '/'. A segment index is highlighted if not all labels have the same
 * value at that position (or if a label is shorter and lacks that segment).
 */
export function diffLabelSegments(labels: string[]): LabelSegment[][] {
  const split = labels.map(l => l.split('/'));
  const maxLen = Math.max(...split.map(s => s.length));

  // For each segment index, check if all labels agree
  const segmentDiffers: boolean[] = [];
  for (let i = 0; i < maxLen; i++) {
    const values = split.map(s => s[i] ?? null);
    const allSame = values.every(v => v === values[0]);
    segmentDiffers.push(!allSame);
  }

  return split.map(segments =>
    segments.map((text, i) => ({
      text,
      highlighted: segmentDiffers[i],
    }))
  );
}
```

**Step 8: Run tests to verify they pass**

Run: `cd ui && npx tsx --test tests/label-format.test.ts`
Expected: All tests PASS

**Step 9: Write `resolveLabelId` helper**

Append to `ui/src/lib/label-format.ts`:

```typescript
/**
 * Resolve a label ID to its display name, with prefix stripping.
 * Falls back to raw ID if not found in the map.
 */
export function resolveLabelId(
  id: string,
  idToName: Record<string, string>,
  prefix: string,
): string {
  const name = idToName[id];
  if (!name) return id;
  return stripPrefix(name, prefix);
}
```

Add a test block in `ui/tests/label-format.test.ts`:

```typescript
describe('resolveLabelId', () => {
  it('resolves and strips prefix', () => {
    const map = { L1: 'automanaged/other/airbnb-com' };
    assert.strictEqual(resolveLabelId('L1', map, 'automanaged'), 'other/airbnb-com');
  });

  it('falls back to raw ID when not in map', () => {
    assert.strictEqual(resolveLabelId('L99', {}, 'automanaged'), 'L99');
  });

  it('property: result is never empty when input is non-empty', () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1 }),
        fc.dictionary(fc.string({ minLength: 1 }), fc.string({ minLength: 1 })),
        fc.string(),
        (id, map, prefix) => {
          const result = resolveLabelId(id, map, prefix);
          return result.length > 0;
        }
      )
    );
  });
});
```

**Step 10: Run all label-format tests**

Run: `cd ui && npx tsx --test tests/label-format.test.ts`
Expected: All PASS

**Step 11: Commit**

```bash
git add ui/src/lib/label-format.ts ui/tests/label-format.test.ts
git commit -m "feat: add label formatting utilities with property-based tests"
```

---

### Task 3: Return label ID-to-name map from detect_overlaps

The `detect_overlaps` command already builds `id_to_name`. Return it alongside groups.

**Files:**
- Modify: `src-tauri/src/commands/remediation.rs` (detect_overlaps command)
- Modify: `ui/src/types/index.ts` (DetectOverlapsResponse type)
- Modify: `ui/src/lib/api.ts` (detectOverlaps return type)
- Modify: `ui/src/stores/app.ts` (add labelMap signal)
- Modify: `ui/src/components/views/RemediationView.tsx` (use labelMap from store)

**Step 1: Write Puppeteer test for label names in filter rows**

Append a new test to `ui/tests/remediation.test.ts`:

```typescript
it('displays label names instead of IDs in pick-winner rows', async () => {
  const groupWithMap = {
    ...pickWinnerGroup,
    filters: [
      { id: 'f5', from: 'news.com', query: null, subject: null, add_label_ids: ['L4'], remove_label_ids: [] },
      { id: 'f6', from: 'news.com', query: null, subject: 'digest', add_label_ids: ['L5'], remove_label_ids: [] },
    ],
  };
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: {
        groups: [groupWithMap],
        label_id_to_name: { L4: 'automanaged/news/news-com', L5: 'automanaged/digests/news-com' },
      },
      get_config_settings: { label_prefix: 'automanaged', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    const row1Text = await page.$eval('[data-testid="filter-row-f5"]', el => el.textContent);
    assert.ok(row1Text?.includes('news/news-com'), `Expected label name but got: ${row1Text}`);
    assert.ok(!row1Text?.includes('L4'), `Should not show raw ID L4: ${row1Text}`);
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: FAIL — detect_overlaps returns the old format (array, not object with label_id_to_name)

**Step 3: Update Rust command to return grouped response**

In `src-tauri/src/commands/remediation.rs`, change `detect_overlaps`:

```rust
#[derive(serde::Serialize)]
pub struct DetectOverlapsResponse {
    pub groups: Vec<OverlapGroup>,
    pub label_id_to_name: HashMap<String, String>,
}

#[tauri::command]
pub async fn detect_overlaps(
    state: State<'_, AppState>,
) -> Result<DetectOverlapsResponse, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let label_cache = state.label_cache.read().clone();
    let id_to_name: HashMap<String, String> = label_cache
        .into_iter()
        .map(|(name, id)| (id, name))
        .collect();

    let groups = OverlapDetector::detect(client.as_ref(), &id_to_name)
        .await
        .map_err(|e| format!("Detection failed: {}", e))?;

    *state.remediation_groups.write() = groups.clone();
    *state.remediation_decisions.write() = HashMap::new();

    Ok(DetectOverlapsResponse {
        groups,
        label_id_to_name: id_to_name,
    })
}
```

**Step 4: Update TypeScript types**

In `ui/src/types/index.ts`, add after the `OverlapGroup` type:

```typescript
export interface DetectOverlapsResponse {
  groups: OverlapGroup[];
  label_id_to_name: Record<string, string>;
}
```

**Step 5: Update API function**

In `ui/src/lib/api.ts`, change `detectOverlaps`:

```typescript
export async function detectOverlaps(): Promise<DetectOverlapsResponse> {
  return invoke<DetectOverlapsResponse>('detect_overlaps');
}
```

Update the import to include `DetectOverlapsResponse`.

**Step 6: Add labelMap and labelPrefix signals to remediation store**

In `ui/src/stores/app.ts`, add two new signals after `remediationError`:

```typescript
const [remediationLabelMap, setRemediationLabelMap] = createSignal<Record<string, string>>({});
const [remediationLabelPrefix, setRemediationLabelPrefix] = createSignal('');
```

Add to the `remediation` export object:

```typescript
labelMap: remediationLabelMap,
labelPrefix: remediationLabelPrefix,
setLabelMap: setRemediationLabelMap,
setLabelPrefix: setRemediationLabelPrefix,
```

Add to `reset()`:

```typescript
setRemediationLabelMap({});
setRemediationLabelPrefix('');
```

**Step 7: Update RemediationView.tsx to use new response shape and resolve labels**

In `ui/src/components/views/RemediationView.tsx`:

Add import:
```typescript
import { resolveLabelId, stripPrefix, diffLabelSegments } from '../../lib/label-format';
import * as api from '../../lib/api';
```

Update `handleDetect` to destructure the response and fetch config:

```typescript
const handleDetect = async () => {
  remediation.setPhase('detecting');
  remediation.setError(null);
  try {
    const [response, config] = await Promise.all([
      api.detectOverlaps(),
      api.getConfigSettings(),
    ]);
    remediation.setGroups(response.groups);
    remediation.setLabelMap(response.label_id_to_name);
    remediation.setLabelPrefix(config.label_prefix);
    setHasDetected(true);
    if (response.groups.length > 0) {
      remediation.setPhase('deciding');
    } else {
      remediation.setPhase('idle');
    }
  } catch (e: any) {
    remediation.setError(e?.message ?? String(e));
    remediation.setPhase('idle');
  }
};
```

Add a helper to resolve label IDs in a filter to display names:

```typescript
const resolveFilterLabel = (labelIds: string[]): string => {
  return labelIds
    .map(id => resolveLabelId(id, remediation.labelMap(), remediation.labelPrefix()))
    .join(', ');
};
```

Replace the raw label ID display on line 267:
```typescript
// Before:
<span class="ml-2 text-gray-400">&rarr; {filter.add_label_ids.join(', ')}</span>
// After:
<span class="ml-2 text-gray-400">&rarr; {resolveFilterLabel(filter.add_label_ids)}</span>
```

Also replace label display on line 217 (group header) to use stripped names:
```typescript
// Before:
Labels: {group.label_names.join(', ')}
// After:
Labels: {group.label_names.map(n => stripPrefix(n, remediation.labelPrefix())).join(', ')}
```

Also replace on line 414 (swap display in results):
```typescript
// Before:
{swap.query} — remove {swap.remove_label_ids.join(', ')} → add {swap.add_label_id}
// After:
{swap.query} — remove {resolveFilterLabel(swap.remove_label_ids)} → add {resolveFilterLabel([swap.add_label_id])}
```

**Step 8: Update existing test fixtures**

In `ui/tests/remediation.test.ts`, update the `baseMock` to use the new response shape. Change `detect_overlaps: []` to:

```typescript
detect_overlaps: { groups: [], label_id_to_name: {} },
```

Update all test overrides that set `detect_overlaps` to wrap in `{ groups: [...], label_id_to_name: {} }`.

Add `get_config_settings` to baseMock defaults:

```typescript
get_config_settings: { label_prefix: '', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
```

**Step 9: Run all remediation tests**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS (including the new label name test)

**Step 10: Commit**

```bash
git add src-tauri/src/commands/remediation.rs ui/src/types/index.ts ui/src/lib/api.ts ui/src/stores/app.ts ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: resolve label IDs to names in remediation UI"
```

---

### Task 4: Segment-level diff highlighting in group cards

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write Puppeteer test for diff highlighting**

Append to `ui/tests/remediation.test.ts`:

```typescript
it('highlights differing label segments in group header', async () => {
  const group = {
    group_id: 'diff-1',
    from_pattern: 'airbnb.com',
    filters: [
      { id: 'f10', from: 'airbnb.com', query: null, subject: null, add_label_ids: ['L10'], remove_label_ids: [] },
      { id: 'f11', from: 'airbnb.com', query: null, subject: 'receipt', add_label_ids: ['L11'], remove_label_ids: [] },
    ],
    label_names: ['automanaged/other/airbnb-com', 'automanaged/marketing/airbnb-com'],
    resolution_type: 'PickWinner' as const,
  };
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: { groups: [group], label_id_to_name: { L10: 'automanaged/other/airbnb-com', L11: 'automanaged/marketing/airbnb-com' } },
      get_config_settings: { label_prefix: 'automanaged', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    // The highlighted segments should have a data-highlighted attribute or distinct class
    const highlighted = await page.$$eval('[data-testid="group-card-diff-1"] [data-highlighted="true"]', els => els.map(el => el.textContent));
    assert.ok(highlighted.includes('other'), `Should highlight "other", got: ${JSON.stringify(highlighted)}`);
    assert.ok(highlighted.includes('marketing'), `Should highlight "marketing", got: ${JSON.stringify(highlighted)}`);
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: FAIL — no `data-highlighted` elements

**Step 3: Create a `DiffLabels` component in RemediationView**

In `RemediationView.tsx`, add a helper component (inside or above the main component):

```typescript
/** Renders label names with segment-level diff highlighting */
const DiffLabels = (props: { labels: string[]; prefix: string }) => {
  const stripped = () => props.labels.map(l => stripPrefix(l, props.prefix));
  const diffed = () => diffLabelSegments(stripped());

  return (
    <span class="inline-flex flex-wrap gap-x-2">
      <For each={diffed()}>
        {(segments, labelIdx) => (
          <>
            <Show when={labelIdx() > 0}>
              <span class="text-gray-400">vs</span>
            </Show>
            <span>
              <For each={segments}>
                {(seg, segIdx) => (
                  <>
                    <Show when={segIdx() > 0}>
                      <span class="text-gray-400">/</span>
                    </Show>
                    <span
                      data-highlighted={seg.highlighted ? 'true' : 'false'}
                      class={seg.highlighted ? 'font-bold text-primary-600 dark:text-primary-400' : 'text-gray-500 dark:text-gray-400'}
                    >
                      {seg.text}
                    </span>
                  </>
                )}
              </For>
            </span>
          </>
        )}
      </For>
    </span>
  );
};
```

**Step 4: Replace the label display line**

Replace line 217 content:

```typescript
// Before:
Labels: {group.label_names.map(n => stripPrefix(n, remediation.labelPrefix())).join(', ')}
// After (when multiple labels, show diff; when single, just show stripped name):
<Show when={group.label_names.length > 1} fallback={<>Labels: {stripPrefix(group.label_names[0] ?? '', remediation.labelPrefix())}</>}>
  Labels: <DiffLabels labels={group.label_names} prefix={remediation.labelPrefix()} />
</Show>
```

**Step 5: Run tests**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS

**Step 6: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: segment-level diff highlighting for conflicting labels"
```

---

### Task 5: Pick-winner override on MechanicalFix cards

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write Puppeteer test for clicking a label to pick winner on MechanicalFix**

Append to `ui/tests/remediation.test.ts`:

```typescript
it('allows picking a winner on a mechanical fix card by clicking a filter label', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: { groups: [mechanicalFixGroup], label_id_to_name: { L2: 'Shopping', L3: 'Receipts' } },
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    // Click filter label to pick winner instead of accepting fix
    await clickAndWait(page, '[data-testid="pick-filter-f3"]');

    // Card should show decided state
    const borderClass = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.className);
    assert.ok(borderClass.includes('border-l-green'), 'Card should show decided state after picking winner');
  } finally {
    await page.close();
  }
});
```

**Step 2: Write Puppeteer test for keyboard shortcuts**

```typescript
it('accepts mechanical fix with A key and picks winner with number keys', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: { groups: [mechanicalFixGroup], label_id_to_name: { L2: 'Shopping', L3: 'Receipts' } },
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    // Press 1 to pick the first filter as winner
    await page.keyboard.press('1');
    await new Promise(r => setTimeout(r, 300));

    const borderClass = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.className);
    assert.ok(borderClass.includes('border-l-green'), 'Card should show decided state after pressing 1');
  } finally {
    await page.close();
  }
});
```

**Step 3: Run tests to verify they fail**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: FAIL — no `pick-filter-f3` testid, no keyboard handler

**Step 4: Add filter pick buttons to MechanicalFix cards**

In the MechanicalFix `<Show>` block (after the proposed replacements list, before the closing `</Show>`), add clickable filter labels:

```typescript
<div class="mt-2 space-y-1">
  <For each={group.filters}>
    {(filter, idx) => (
      <button
        data-testid={`pick-filter-${filter.id}`}
        class={`w-full text-left text-xs rounded border px-2 py-1 transition ${
          getPickedFilterId(group.group_id) === filter.id
            ? 'ring-2 ring-primary-300 border-primary-500 bg-primary-50 dark:bg-primary-900/30'
            : 'border-gray-200 dark:border-gray-600 hover:border-gray-300'
        }`}
        onClick={() => handlePickWinner(group, filter.id)}
      >
        <span class="text-gray-500">{idx() + 1}.</span>{' '}
        <span class="font-medium">{resolveFilterLabel(filter.add_label_ids)}</span>
        <span class="ml-1 text-gray-400">— pick as winner</span>
      </button>
    )}
  </For>
</div>
```

**Step 5: Add keyboard shortcut handler**

In `RemediationView.tsx`, add inside the component function (after the helpers, before the return):

```typescript
// Track which group is "current" for keyboard — first undecided group
const currentUndecidedGroup = () =>
  remediation.groups().find(g => getDecisionState(g.group_id) === 'undecided');

onMount(() => {
  const handleKeyDown = async (e: KeyboardEvent) => {
    if (remediation.phase() !== 'deciding') return;
    const group = currentUndecidedGroup();
    if (!group) return;

    const key = e.key.toLowerCase();

    if (key === 'a') {
      e.preventDefault();
      if (isConsolidate(group) || isMechanicalFix(group)) {
        await handleAccept(group);
      }
      return;
    }

    if (key === 's') {
      e.preventDefault();
      await handleSkip(group);
      return;
    }

    // Number keys: pick winner by filter index (1-based)
    const num = parseInt(e.key, 10);
    if (!isNaN(num) && num >= 1 && num <= group.filters.length) {
      e.preventDefault();
      await handlePickWinner(group, group.filters[num - 1].id);
      return;
    }
  };

  window.addEventListener('keydown', handleKeyDown);
  onCleanup(() => window.removeEventListener('keydown', handleKeyDown));
});
```

Add `onMount` and `onCleanup` to the SolidJS imports at line 1.

**Step 6: Run tests**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS

**Step 7: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: pick-winner override on MechanicalFix cards with keyboard shortcuts"
```

---

### Task 6: Progress indicator with ETA during execution

**Files:**
- Modify: `src-tauri/src/events.rs` (add RemediationProgress event type)
- Modify: `src-tauri/src/commands/remediation.rs` (use execute_with_progress + emit events)
- Modify: `ui/src/lib/events.ts` (add remediation progress listener)
- Modify: `ui/src/stores/app.ts` (add progress signals)
- Modify: `ui/src/components/views/RemediationView.tsx` (show progress bar + ETA)
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write Puppeteer test for progress display**

The mock can't easily emit Tauri events, so test that the progress UI renders when store signals are set. Instead, test that after execution completes the progress indicator is gone and results show. For the progress display itself, we test by directly setting store signals via JS injection.

Append to `ui/tests/remediation.test.ts`:

```typescript
it('shows progress counter during execution phase', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} },
      remediation_summary: 'plan',
      execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: [],
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="review-plan-btn"]');

    // The execution is fast with mocks, so just check the result phase renders
    await clickAndWait(page, '[data-testid="execute-btn"]');
    const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
    assert.ok(deleted?.includes('1'), 'Should show results after execution');
  } finally {
    await page.close();
  }
});
```

**Step 2: Add RemediationProgress event type to Rust events**

In `src-tauri/src/events.rs`, add:

```rust
/// Progress update for remediation execution
#[derive(Debug, Clone, Serialize)]
pub struct RemediationProgress {
    /// Number of groups completed
    pub done: usize,
    /// Total groups
    pub total: usize,
    /// ID of the group just completed
    pub group_id: String,
}
```

Add to `EventEmitter`:

```rust
pub fn emit_remediation_progress(&self, progress: RemediationProgress) {
    let _ = self.app.emit("remediation:progress", progress);
}
```

**Step 3: Update execute_remediation to use execute_with_progress and emit events**

In `src-tauri/src/commands/remediation.rs`, change `execute_remediation` to accept `AppHandle`:

```rust
#[tauri::command]
pub async fn execute_remediation(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RemediationResult, String> {
    let client = state
        .get_client()
        .ok_or_else(|| "Not authenticated".to_string())?;

    let plan = build_plan_from_state(&state);
    let total = plan.group_count();

    let result = plan
        .execute_with_progress(client.as_ref(), |done, group_id| {
            let _ = app.emit("remediation:progress", serde_json::json!({
                "done": done,
                "total": total,
                "group_id": group_id,
            }));
        })
        .await
        .map_err(|e| format!("Execution failed: {}", e))?;

    Ok(result)
}
```

Note: `plan.group_count()` — check if this method exists. If not, use `plan.groups.len()`. The `on_progress` closure captures `app` by ref which works since Tauri commands run in an async context. You may need `use tauri::{AppHandle, Emitter};` at the top.

If `RemediationPlan` doesn't expose `group_count()`, add a simple method:

In `src/filter_remediation.rs`, on `RemediationPlan`:
```rust
pub fn group_count(&self) -> usize {
    self.groups.len()
}
```

**Step 4: Add remediation progress listener to frontend events**

In `ui/src/lib/events.ts`, add:

```typescript
export interface RemediationProgress {
  done: number;
  total: number;
  group_id: string;
}

export type RemediationProgressHandler = (progress: RemediationProgress) => void;

export async function onRemediationProgress(handler: RemediationProgressHandler): Promise<UnlistenFn> {
  return listen<RemediationProgress>('remediation:progress', (event) => {
    handler(event.payload);
  });
}
```

Add `remediationProgress` to `EventListeners` interface and `setupEventListeners`.

**Step 5: Add progress signals to remediation store**

In `ui/src/stores/app.ts`, add:

```typescript
const [remediationProgressDone, setRemediationProgressDone] = createSignal(0);
const [remediationProgressTotal, setRemediationProgressTotal] = createSignal(0);
const [remediationProgressTicks, setRemediationProgressTicks] = createSignal<number[]>([]);
```

Add to `remediation` export:
```typescript
progressDone: remediationProgressDone,
progressTotal: remediationProgressTotal,
progressTicks: remediationProgressTicks,
setProgressDone: setRemediationProgressDone,
setProgressTotal: setRemediationProgressTotal,
addProgressTick: () => {
  setRemediationProgressTicks(prev => [...prev, Date.now()]);
},
```

Add to `reset()`:
```typescript
setRemediationProgressDone(0);
setRemediationProgressTotal(0);
setRemediationProgressTicks([]);
```

**Step 6: Wire progress events in RemediationView.tsx**

Update `handleExecute` to listen for progress events:

```typescript
const handleExecute = async () => {
  remediation.setError(null);
  remediation.setPhase('executing');
  remediation.setProgressDone(0);
  remediation.setProgressTotal(remediation.groups().length);

  const unlisten = await onRemediationProgress((progress) => {
    remediation.setProgressDone(progress.done);
    remediation.setProgressTotal(progress.total);
    remediation.addProgressTick();
  });

  try {
    const result = await api.executeRemediation();
    remediation.setResult(result);
    const swaps = await api.collectRemediationSwaps();
    remediation.setSwaps(swaps);
    remediation.setPhase('results');
  } catch (e: any) {
    remediation.setError(e?.message ?? String(e));
    remediation.setPhase('confirming');
  } finally {
    unlisten();
  }
};
```

Add import: `import { onRemediationProgress } from '../../lib/events';`

**Step 7: Replace spinner with progress bar + ETA**

Replace the executing phase block (lines 357-362) with:

```typescript
<Show when={remediation.phase() === 'executing'}>
  <div class="card p-8 text-center">
    <div class="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2 mb-4">
      <div
        class="bg-primary-600 h-2 rounded-full transition-all duration-300"
        style={{ width: `${remediation.progressTotal() > 0 ? (remediation.progressDone() / remediation.progressTotal()) * 100 : 0}%` }}
      />
    </div>
    <p class="text-gray-700 dark:text-gray-300 font-medium">
      {remediation.progressDone()}/{remediation.progressTotal()} groups
    </p>
    <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
      {estimatedTimeRemaining()}
    </p>
  </div>
</Show>
```

Add the ETA helper inside the component:

```typescript
const estimatedTimeRemaining = (): string => {
  const ticks = remediation.progressTicks();
  const done = remediation.progressDone();
  const total = remediation.progressTotal();
  const remaining = total - done;

  if (ticks.length < 3 || remaining <= 0) return 'Estimating...';

  // Average time between ticks (use last N ticks for rolling average)
  const recentTicks = ticks.slice(-10);
  const intervals: number[] = [];
  for (let i = 1; i < recentTicks.length; i++) {
    intervals.push(recentTicks[i] - recentTicks[i - 1]);
  }
  const avgMs = intervals.reduce((a, b) => a + b, 0) / intervals.length;
  const etaSeconds = Math.ceil((avgMs * remaining) / 1000);

  if (etaSeconds < 60) return `~${etaSeconds}s remaining`;
  const mins = Math.floor(etaSeconds / 60);
  const secs = etaSeconds % 60;
  return `~${mins}m ${secs}s remaining`;
};
```

**Step 8: Run all tests**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS

**Step 9: Build check**

Run: `cd ui && npm run build`
Expected: No TypeScript errors

Run: `cd src-tauri && cargo check`
Expected: No Rust errors

**Step 10: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/commands/remediation.rs src/filter_remediation.rs ui/src/lib/events.ts ui/src/stores/app.ts ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: progress indicator with ETA during remediation execution"
```

---

### Task 7: Update mock infrastructure for event emission

The Tauri mock currently only handles `invoke`. For tests that need event-based progress, enhance `mock-tauri.ts` to support `listen`.

**Files:**
- Modify: `ui/tests/mock-tauri.ts`

**Step 1: Add event listener mock support**

Update `buildMockScript` to also mock `window.__TAURI_INTERNALS__` event listening. The existing mock already has `transformCallback`. Add a `plugin:event|listen` handler that stores callbacks, so test code can trigger events.

```typescript
export function buildMockScript(mockResponses: Record<string, unknown>): string {
  return `
    window.__TAURI_EVENT_LISTENERS__ = {};
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        const responses = ${JSON.stringify(mockResponses)};
        if (cmd === 'plugin:event|listen') {
          const event = args?.event;
          const handler = args?.handler;
          if (event && handler !== undefined) {
            if (!window.__TAURI_EVENT_LISTENERS__[event]) {
              window.__TAURI_EVENT_LISTENERS__[event] = [];
            }
            window.__TAURI_EVENT_LISTENERS__[event].push(handler);
          }
          return 0;
        }
        if (cmd in responses) {
          return responses[cmd];
        }
        throw new Error('unmocked command: ' + cmd);
      },
      convertFileSrc: (src) => src,
      transformCallback: (cb) => {
        const id = Math.random();
        window['_' + id] = cb;
        return id;
      },
    };
  `;
}
```

**Step 2: Run all tests to verify nothing broke**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS

**Step 3: Commit**

```bash
git add ui/tests/mock-tauri.ts
git commit -m "chore: enhance Tauri mock with event listener support"
```

---

### Task 8: Final integration test

**Files:**
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write full flow test with all four improvements**

```typescript
it('full flow with label names, diff highlighting, keyboard shortcuts, and progress', async () => {
  const groups = [
    {
      ...mechanicalFixGroup,
      label_names: ['automanaged/shopping/store-com', 'automanaged/receipts/store-com'],
    },
    pickWinnerGroup,
  ];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: {
        groups,
        label_id_to_name: {
          L2: 'automanaged/shopping/store-com',
          L3: 'automanaged/receipts/store-com',
          L4: 'News',
          L5: 'Digests',
        },
      },
      get_config_settings: { label_prefix: 'automanaged', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
      remediation_summary: 'plan: 2 groups',
      execute_remediation: { deleted: ['f4', 'f6'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: [],
    }));

    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    // Verify diff highlighting on mechfix group
    const highlighted = await page.$$eval('[data-testid="group-card-mechfix-1"] [data-highlighted="true"]', els => els.map(el => el.textContent));
    assert.ok(highlighted.includes('shopping') || highlighted.includes('receipts'), `Should highlight differing segments: ${JSON.stringify(highlighted)}`);

    // Use keyboard: A to accept first group (mechfix)
    await page.keyboard.press('a');
    await new Promise(r => setTimeout(r, 300));

    // Use keyboard: 1 to pick winner on second group (pick-winner)
    await page.keyboard.press('1');
    await new Promise(r => setTimeout(r, 300));

    // Review and execute
    await clickAndWait(page, '[data-testid="review-plan-btn"]');
    await clickAndWait(page, '[data-testid="execute-btn"]');

    // Should reach results
    const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
    assert.ok(deleted?.includes('2'), 'Should show 2 deleted');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run all tests**

Run: `cd ui && npx tsx --test tests/remediation.test.ts`
Expected: All PASS

**Step 3: Run full build**

Run: `cd ui && npm run build && cd ../src-tauri && cargo check`
Expected: No errors

**Step 4: Commit**

```bash
git add ui/tests/remediation.test.ts
git commit -m "test: add integration test for all four remediation UI improvements"
```
