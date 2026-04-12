# Remediation UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the Tauri GUI frontend for the existing filter remediation flow using TDD with Puppeteer against the Vite dev server.

**Architecture:** Phased single-page view (detect -> decide -> confirm -> results) as a new top-level `RemediationView`. All overlap groups shown as cards for batch decisions. Tauri `invoke` calls mocked in tests via `window.__TAURI_INTERNALS__`.

**Tech Stack:** SolidJS, TypeScript, Tailwind CSS, Puppeteer, `node:test`

---

### Task 1: Test Infrastructure — Puppeteer + node:test + Tauri Mock

**Files:**
- Create: `ui/tests/setup.ts`
- Create: `ui/tests/mock-tauri.ts`
- Create: `ui/tests/remediation.test.ts` (initial skeleton)
- Modify: `ui/package.json` (add puppeteer devDep + test script)

**Step 1: Install puppeteer**

Run: `cd ui && npm install --save-dev puppeteer`

**Step 2: Create the Tauri mock helper**

Create `ui/tests/mock-tauri.ts`:

```typescript
/**
 * Injects a mock for window.__TAURI_INTERNALS__.invoke into the page.
 * Call this via page.evaluate() before each test.
 *
 * mockResponses is a Record<commandName, responseData>.
 * Any command not in the map rejects with "unmocked command: <name>".
 */
export function buildMockScript(mockResponses: Record<string, unknown>): string {
  return `
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        const responses = ${JSON.stringify(mockResponses)};
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

**Step 3: Create the test setup helper**

Create `ui/tests/setup.ts`:

```typescript
import puppeteer, { type Browser, type Page } from 'puppeteer';

const VITE_URL = 'http://localhost:5173';

let browser: Browser;

export async function setupBrowser(): Promise<Browser> {
  browser = await puppeteer.launch({ headless: true });
  return browser;
}

export async function teardownBrowser(): Promise<void> {
  if (browser) await browser.close();
}

export async function newPage(): Promise<Page> {
  const page = await browser.newPage();
  return page;
}

/**
 * Navigate to the app with mock Tauri internals injected BEFORE the app JS loads.
 * Uses page.evaluateOnNewDocument so the mock is available when SolidJS mounts.
 */
export async function navigateWithMock(
  page: Page,
  mockScript: string
): Promise<void> {
  await page.evaluateOnNewDocument(mockScript);
  await page.goto(VITE_URL, { waitUntil: 'networkidle0' });
}

/**
 * Wait for a selector and click it.
 */
export async function clickAndWait(
  page: Page,
  selector: string,
  waitMs = 500
): Promise<void> {
  await page.waitForSelector(selector, { timeout: 5000 });
  await page.click(selector);
  // Give SolidJS time to re-render
  await new Promise(r => setTimeout(r, waitMs));
}
```

**Step 4: Create initial test skeleton**

Create `ui/tests/remediation.test.ts`:

```typescript
import { describe, it, before, after } from 'node:test';
import assert from 'node:assert';
import { setupBrowser, teardownBrowser, newPage, navigateWithMock, clickAndWait } from './setup.ts';
import { buildMockScript } from './mock-tauri.ts';
import type { Page } from 'puppeteer';

// Default mock: authenticated user, no-op for most commands
function baseMock(overrides: Record<string, unknown> = {}): string {
  return buildMockScript({
    check_auth_status: { authenticated: true, email: 'test@example.com', credentials_path: '', credentials_exist: true, token_exists: true },
    initialize_client: true,
    detect_overlaps: [],
    submit_group_decision: null,
    execute_remediation: { deleted: [], created: [], skipped: 0, errors: [] },
    remediation_summary: 'Remediation plan: 0 groups, 0 deletions, 0 creations, 0 skipped',
    collect_remediation_swaps: [],
    apply_remediation_swaps: { messages_relabeled: 0, messages_failed: 0, errors: [] },
    ...overrides,
  });
}

describe('Remediation UI', () => {
  before(async () => { await setupBrowser(); });
  after(async () => { await teardownBrowser(); });

  it('should render without crashing', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock());
      const body = await page.$('body');
      assert.ok(body, 'Page body should exist');
    } finally {
      await page.close();
    }
  });
});
```

**Step 5: Add test script to package.json**

In `ui/package.json`, add to `"scripts"`:

```json
"test": "node --import tsx --test tests/remediation.test.ts"
```

Also add devDependency: `"tsx": "^4.7.0"` (to run TypeScript tests directly).

Run: `cd ui && npm install --save-dev tsx`

**Step 6: Verify the test runner works**

Run (requires Vite dev server already running on port 5173):

```bash
cd ui && npm run dev &
sleep 3
npm test
```

Expected: 1 test passes (renders without crashing). Kill the dev server after.

**Step 7: Commit**

```bash
git add ui/tests/ ui/package.json ui/package-lock.json
git commit -m "test: add Puppeteer test infrastructure with Tauri invoke mock"
```

---

### Task 2: Types — Add Remediation Types to TypeScript

**Files:**
- Modify: `ui/src/types/index.ts:312` (add types + extend AppView)

**Step 1: Write a failing test**

Add to `ui/tests/remediation.test.ts`:

```typescript
it('navigates to remediation view via sidebar', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock());
    // Click the Remediation sidebar item
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    // Should see the remediation view
    const heading = await page.$eval('[data-testid="remediation-view"]', el => el.textContent);
    assert.ok(heading, 'Remediation view should be rendered');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npm test`
Expected: FAIL — `nav-remediation` selector not found.

**Step 3: Add types to `ui/src/types/index.ts`**

Append before the `AppView` type (at end of file):

```typescript
// Remediation types
export interface OverlapFilter {
  id: string;
  from: string | null;
  query: string | null;
  subject: string | null;
  add_label_ids: string[];
  remove_label_ids: string[];
}

export type ResolutionType =
  | { Consolidate: { keep_filter_id: string; remove_filter_ids: string[] } }
  | { MechanicalFix: { proposed_replacements: FilterRule[] } }
  | 'PickWinner';

export interface FilterRule {
  id: string | null;
  name: string;
  from_pattern: string | null;
  is_specific_sender: boolean;
  excluded_senders: string[];
  subject_keywords: string[];
  excluded_subject_patterns: string[];
  target_label_id: string;
  should_archive: boolean;
}

export interface OverlapGroup {
  group_id: string;
  from_pattern: string;
  filters: OverlapFilter[];
  label_names: string[];
  resolution_type: ResolutionType;
}

export type GroupDecision =
  | { Consolidate: { keep_filter_id: string; remove_filter_ids: string[] } }
  | { ReplaceWithExclusive: { replacement_filters: FilterRule[] } }
  | { KeepOne: { keep_filter_id: string } }
  | 'Skip'
  | { Rescan: { from_pattern: string } };

export interface RemediationResult {
  deleted: string[];
  created: string[];
  skipped: number;
  errors: string[];
}

export interface LabelSwap {
  query: string;
  add_label_id: string;
  remove_label_ids: string[];
}

export interface ApplyResult {
  messages_relabeled: number;
  messages_failed: number;
  errors: string[];
}

export type RemediationPhase = 'idle' | 'detecting' | 'deciding' | 'confirming' | 'executing' | 'results';
```

Change the `AppView` line from:

```typescript
export type AppView = 'auth' | 'scan' | 'review' | 'filters' | 'coverage' | 'editor' | 'settings';
```

to:

```typescript
export type AppView = 'auth' | 'scan' | 'review' | 'filters' | 'coverage' | 'editor' | 'remediation' | 'settings';
```

**Step 4: Commit** (test still fails, that's expected — types alone don't make UI appear)

```bash
git add ui/src/types/index.ts
git commit -m "feat: add remediation TypeScript types and AppView entry"
```

---

### Task 3: API + Store + Navigation Wiring

**Files:**
- Modify: `ui/src/lib/api.ts` (add 6 remediation API functions)
- Modify: `ui/src/stores/app.ts` (add remediation store section)
- Modify: `ui/src/components/layout/Sidebar.tsx:12-18` (add nav item)
- Modify: `ui/src/App.tsx` (add Match + import)
- Create: `ui/src/components/views/RemediationView.tsx` (minimal stub)

**Step 1: Add API functions to `ui/src/lib/api.ts`**

Append before the `// ============ Settings Commands ============` section:

```typescript
// ============ Remediation Commands ============

export async function detectOverlaps(): Promise<OverlapGroup[]> {
  return invoke<OverlapGroup[]>('detect_overlaps');
}

export async function submitGroupDecision(groupId: string, decision: GroupDecision): Promise<void> {
  return invoke<void>('submit_group_decision', { groupId, decision });
}

export async function executeRemediation(): Promise<RemediationResult> {
  return invoke<RemediationResult>('execute_remediation');
}

export async function remediationSummary(): Promise<string> {
  return invoke<string>('remediation_summary');
}

export async function collectRemediationSwaps(): Promise<LabelSwap[]> {
  return invoke<LabelSwap[]>('collect_remediation_swaps');
}

export async function applyRemediationSwaps(): Promise<ApplyResult> {
  return invoke<ApplyResult>('apply_remediation_swaps');
}
```

Add the necessary imports to the import block at the top of `api.ts`:

```typescript
import type {
  // ... existing imports ...
  OverlapGroup,
  GroupDecision,
  RemediationResult,
  LabelSwap,
  ApplyResult,
} from '../types';
```

**Step 2: Add remediation store to `ui/src/stores/app.ts`**

Add imports at top:

```typescript
import type {
  // ... existing imports ...
  OverlapGroup,
  GroupDecision,
  RemediationResult,
  RemediationPhase,
  LabelSwap,
  ApplyResult,
} from '../types';
```

Add before `// ============ Coverage State ============`:

```typescript
// ============ Remediation State ============

const [remediationPhase, setRemediationPhase] = createSignal<RemediationPhase>('idle');
const [remediationGroups, setRemediationGroups] = createSignal<OverlapGroup[]>([]);
const [remediationDecisions, setRemediationDecisions] = createSignal<Record<string, GroupDecision>>({});
const [remediationSummary, setRemediationSummary] = createSignal('');
const [remediationResult, setRemediationResult] = createSignal<RemediationResult | null>(null);
const [remediationSwaps, setRemediationSwaps] = createSignal<LabelSwap[]>([]);
const [remediationApplyResult, setRemediationApplyResult] = createSignal<ApplyResult | null>(null);
const [remediationError, setRemediationError] = createSignal<string | null>(null);

export const remediation = {
  phase: remediationPhase,
  groups: remediationGroups,
  decisions: remediationDecisions,
  summary: remediationSummary,
  result: remediationResult,
  swaps: remediationSwaps,
  applyResult: remediationApplyResult,
  error: remediationError,
  setPhase: setRemediationPhase,
  setGroups: setRemediationGroups,
  setDecisions: setRemediationDecisions,
  setSummary: setRemediationSummary,
  setResult: setRemediationResult,
  setSwaps: setRemediationSwaps,
  setApplyResult: setRemediationApplyResult,
  setError: setRemediationError,
  setDecision: (groupId: string, decision: GroupDecision) => {
    setRemediationDecisions(prev => ({ ...prev, [groupId]: decision }));
  },
  decidedCount: createMemo(() => Object.keys(remediationDecisions()).length),
  allDecided: createMemo(() => {
    const groups = remediationGroups();
    const decisions = remediationDecisions();
    return groups.length > 0 && groups.every(g => g.group_id in decisions);
  }),
  reset: () => {
    setRemediationPhase('idle');
    setRemediationGroups([]);
    setRemediationDecisions({});
    setRemediationSummary('');
    setRemediationResult(null);
    setRemediationSwaps([]);
    setRemediationApplyResult(null);
    setRemediationError(null);
  },
};
```

Add `remediation.reset()` to the `resetAllState` function.

**Step 3: Add sidebar nav item**

In `ui/src/components/layout/Sidebar.tsx`, change the `navItems` array from:

```typescript
const navItems: NavItem[] = [
  { id: 'scan', label: 'Scan', icon: '📧' },
  { id: 'review', label: 'Review', icon: '✓', badge: () => review.undecidedCount() || null },
  { id: 'filters', label: 'Filters', icon: '🔧' },
  { id: 'coverage', label: 'Coverage', icon: '📊' },
  { id: 'editor', label: 'Editor', icon: '✏️' },
];
```

to:

```typescript
const navItems: NavItem[] = [
  { id: 'scan', label: 'Scan', icon: '📧' },
  { id: 'review', label: 'Review', icon: '✓', badge: () => review.undecidedCount() || null },
  { id: 'filters', label: 'Filters', icon: '🔧' },
  { id: 'remediation', label: 'Remediation', icon: '🔀' },
  { id: 'coverage', label: 'Coverage', icon: '📊' },
  { id: 'editor', label: 'Editor', icon: '✏️' },
];
```

Add `data-testid` attribute to the nav button. Change:

```tsx
<button
  onClick={() => navigation.goTo(item.id)}
  class="w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-colors"
```

to:

```tsx
<button
  data-testid={`nav-${item.id}`}
  onClick={() => navigation.goTo(item.id)}
  class="w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-colors"
```

**Step 4: Create minimal RemediationView stub**

Create `ui/src/components/views/RemediationView.tsx`:

```tsx
import { Component } from 'solid-js';

const RemediationView: Component = () => {
  return (
    <div data-testid="remediation-view" class="max-w-4xl mx-auto space-y-6">
      <h2 class="text-xl font-bold text-gray-900 dark:text-white">
        Filter Remediation
      </h2>
    </div>
  );
};

export default RemediationView;
```

**Step 5: Wire into App.tsx**

Add import:

```typescript
import RemediationView from './components/views/RemediationView';
```

Add `<Match>` block after the coverage match (before editor):

```tsx
<Match when={navigation.currentView() === 'remediation'}>
  <RemediationView />
</Match>
```

**Step 6: Run test to verify it passes**

Run: `cd ui && npm test`
Expected: Both tests pass — the skeleton renders and navigation works.

**Step 7: Commit**

```bash
git add ui/src/lib/api.ts ui/src/stores/app.ts ui/src/components/layout/Sidebar.tsx ui/src/App.tsx ui/src/components/views/RemediationView.tsx
git commit -m "feat: wire remediation API, store, navigation, and view stub"
```

---

### Task 4: Detect Phase UI

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing tests**

Add to `ui/tests/remediation.test.ts`:

```typescript
it('shows detect button in idle phase', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock());
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    const btn = await page.$('[data-testid="detect-btn"]');
    assert.ok(btn, 'Detect Overlaps button should be visible');
    const text = await page.$eval('[data-testid="detect-btn"]', el => el.textContent);
    assert.ok(text?.includes('Detect Overlaps'), 'Button should say Detect Overlaps');
  } finally {
    await page.close();
  }
});

it('transitions to decide phase after detecting groups', async () => {
  const mockGroups = [
    {
      group_id: 'group-1',
      from_pattern: 'example.com',
      filters: [
        { id: 'f1', from: 'example.com', query: null, subject: null, add_label_ids: ['L1'], remove_label_ids: [] },
        { id: 'f2', from: 'example.com', query: null, subject: 'promo', add_label_ids: ['L1'], remove_label_ids: [] },
      ],
      label_names: ['Newsletters'],
      resolution_type: { Consolidate: { keep_filter_id: 'f1', remove_filter_ids: ['f2'] } },
    },
  ];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: mockGroups }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const heading = await page.$eval('[data-testid="decide-header"]', el => el.textContent);
    assert.ok(heading?.includes('1'), 'Should show 1 overlap group found');
  } finally {
    await page.close();
  }
});

it('shows empty state when no overlaps detected', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const msg = await page.$eval('[data-testid="no-overlaps"]', el => el.textContent);
    assert.ok(msg?.includes('No overlapping filters'), 'Should show no overlaps message');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run tests to verify they fail**

Run: `cd ui && npm test`
Expected: 3 new tests FAIL — `detect-btn` not found.

**Step 3: Implement detect phase in RemediationView**

Replace `ui/src/components/views/RemediationView.tsx` with:

```tsx
import { Component, Show } from 'solid-js';
import { remediation } from '../../stores/app';
import * as api from '../../lib/api';

const RemediationView: Component = () => {
  const handleDetect = async () => {
    remediation.setError(null);
    remediation.setPhase('detecting');
    try {
      const groups = await api.detectOverlaps();
      remediation.setGroups(groups);
      remediation.setDecisions({});
      if (groups.length === 0) {
        remediation.setPhase('idle');
      } else {
        remediation.setPhase('deciding');
      }
    } catch (e) {
      remediation.setError(e instanceof Error ? e.message : String(e));
      remediation.setPhase('idle');
    }
  };

  return (
    <div data-testid="remediation-view" class="max-w-4xl mx-auto space-y-6">
      <h2 class="text-xl font-bold text-gray-900 dark:text-white">
        Filter Remediation
      </h2>

      {/* Error */}
      <Show when={remediation.error()}>
        <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{remediation.error()}</p>
        </div>
      </Show>

      {/* Phase: Idle */}
      <Show when={remediation.phase() === 'idle'}>
        <div class="card p-6">
          <p class="text-gray-600 dark:text-gray-400 mb-4">
            Scan your Gmail filters for overlapping rules and resolve conflicts.
          </p>
          <button
            data-testid="detect-btn"
            onClick={handleDetect}
            class="btn-primary"
          >
            Detect Overlaps
          </button>

          {/* Empty result after detection */}
          <Show when={remediation.groups().length === 0 && !remediation.error() && remediation.phase() === 'idle'}>
            {/* This won't show on first load because we need a way to know detection ran.
                We'll use a separate signal below. */}
          </Show>
        </div>
      </Show>

      {/* Phase: Detecting (loading) */}
      <Show when={remediation.phase() === 'detecting'}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Scanning filters for overlaps...</p>
        </div>
      </Show>

      {/* Phase: Deciding */}
      <Show when={remediation.phase() === 'deciding'}>
        <h3 data-testid="decide-header" class="text-lg font-semibold text-gray-900 dark:text-white">
          {remediation.groups().length} overlap group{remediation.groups().length !== 1 ? 's' : ''} found
        </h3>
      </Show>
    </div>
  );
};

export default RemediationView;
```

Wait — the "no overlaps" empty state needs a tweak. When detect returns `[]`, phase goes back to `idle`, but we need to distinguish "never detected" from "detected and found nothing". Add a local signal:

```tsx
import { Component, Show, createSignal } from 'solid-js';
import { remediation } from '../../stores/app';
import * as api from '../../lib/api';

const RemediationView: Component = () => {
  const [hasDetected, setHasDetected] = createSignal(false);

  const handleDetect = async () => {
    remediation.setError(null);
    remediation.setPhase('detecting');
    try {
      const groups = await api.detectOverlaps();
      remediation.setGroups(groups);
      remediation.setDecisions({});
      setHasDetected(true);
      if (groups.length === 0) {
        remediation.setPhase('idle');
      } else {
        remediation.setPhase('deciding');
      }
    } catch (e) {
      remediation.setError(e instanceof Error ? e.message : String(e));
      remediation.setPhase('idle');
    }
  };

  return (
    <div data-testid="remediation-view" class="max-w-4xl mx-auto space-y-6">
      <h2 class="text-xl font-bold text-gray-900 dark:text-white">
        Filter Remediation
      </h2>

      {/* Error */}
      <Show when={remediation.error()}>
        <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{remediation.error()}</p>
        </div>
      </Show>

      {/* Phase: Idle */}
      <Show when={remediation.phase() === 'idle'}>
        <div class="card p-6">
          <p class="text-gray-600 dark:text-gray-400 mb-4">
            Scan your Gmail filters for overlapping rules and resolve conflicts.
          </p>
          <button
            data-testid="detect-btn"
            onClick={handleDetect}
            class="btn-primary"
          >
            Detect Overlaps
          </button>
        </div>

        <Show when={hasDetected() && remediation.groups().length === 0}>
          <div data-testid="no-overlaps" class="card p-6 text-center">
            <p class="text-gray-500 dark:text-gray-400">
              No overlapping filters found. Your filters are clean!
            </p>
          </div>
        </Show>
      </Show>

      {/* Phase: Detecting (loading) */}
      <Show when={remediation.phase() === 'detecting'}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Scanning filters for overlaps...</p>
        </div>
      </Show>

      {/* Phase: Deciding */}
      <Show when={remediation.phase() === 'deciding'}>
        <h3 data-testid="decide-header" class="text-lg font-semibold text-gray-900 dark:text-white">
          {remediation.groups().length} overlap group{remediation.groups().length !== 1 ? 's' : ''} found
        </h3>
      </Show>
    </div>
  );
};

export default RemediationView;
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 5 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement detect phase UI with empty state handling"
```

---

### Task 5: Consolidate Card

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing tests**

Add to `ui/tests/remediation.test.ts`:

```typescript
// Shared fixture for consolidate group
const consolidateGroup = {
  group_id: 'consolidate-1',
  from_pattern: 'example.com',
  filters: [
    { id: 'f1', from: 'example.com', query: null, subject: null, add_label_ids: ['L1'], remove_label_ids: [] },
    { id: 'f2', from: 'example.com', query: null, subject: 'promo', add_label_ids: ['L1'], remove_label_ids: [] },
  ],
  label_names: ['Newsletters'],
  resolution_type: { Consolidate: { keep_filter_id: 'f1', remove_filter_ids: ['f2'] } },
};

it('renders consolidate card with accept and skip buttons', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [consolidateGroup] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const card = await page.$('[data-testid="group-card-consolidate-1"]');
    assert.ok(card, 'Consolidate card should be rendered');
    const acceptBtn = await page.$('[data-testid="accept-consolidate-1"]');
    assert.ok(acceptBtn, 'Accept button should exist');
    const skipBtn = await page.$('[data-testid="skip-consolidate-1"]');
    assert.ok(skipBtn, 'Skip button should exist');
  } finally {
    await page.close();
  }
});

it('marks consolidate card as decided after accepting', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [consolidateGroup] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    const card = await page.$('[data-testid="group-card-consolidate-1"]');
    const borderClass = await page.$eval('[data-testid="group-card-consolidate-1"]', el => el.className);
    assert.ok(borderClass.includes('border-l-green'), 'Card should have green left border when decided');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run tests to verify they fail**

Run: `cd ui && npm test`
Expected: FAIL — `group-card-consolidate-1` not found.

**Step 3: Add card rendering to the deciding phase**

In `RemediationView.tsx`, add a `For` loop inside the deciding `<Show>` block, after the `<h3>` header. Add imports for `For` and the needed types:

```tsx
import { Component, Show, For, createSignal, createMemo } from 'solid-js';
import { remediation } from '../../stores/app';
import * as api from '../../lib/api';
import type { OverlapGroup, GroupDecision } from '../../types';
```

Inside the deciding phase, after the header `<h3>`:

```tsx
<div class="space-y-4">
  <For each={remediation.groups()}>
    {(group) => {
      const decision = createMemo(() => remediation.decisions()[group.group_id]);
      const isDecided = createMemo(() => decision() !== undefined);
      const isSkipped = createMemo(() => decision() === 'Skip');

      const borderColor = () => {
        if (isSkipped()) return 'border-l-yellow-400';
        if (isDecided()) return 'border-l-green-500';
        return 'border-l-gray-300 dark:border-l-gray-600';
      };

      const handleAcceptConsolidate = async (group: OverlapGroup) => {
        const res = group.resolution_type;
        if (typeof res === 'object' && 'Consolidate' in res) {
          const decision: GroupDecision = {
            Consolidate: {
              keep_filter_id: res.Consolidate.keep_filter_id,
              remove_filter_ids: res.Consolidate.remove_filter_ids,
            },
          };
          remediation.setDecision(group.group_id, decision);
          await api.submitGroupDecision(group.group_id, decision);
        }
      };

      const handleSkip = async (groupId: string) => {
        remediation.setDecision(groupId, 'Skip');
        await api.submitGroupDecision(groupId, 'Skip');
      };

      const isConsolidate = () =>
        typeof group.resolution_type === 'object' && 'Consolidate' in group.resolution_type;

      return (
        <div
          data-testid={`group-card-${group.group_id}`}
          class={`card p-4 border-l-4 ${borderColor()}`}
        >
          <div class="flex items-start justify-between">
            <div>
              <h4 class="font-semibold text-gray-900 dark:text-white">
                {group.from_pattern}
              </h4>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Labels: {group.label_names.join(', ')}
              </p>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                {group.filters.length} filters
              </p>
            </div>
            <Show when={isDecided()}>
              <span class={`text-xs px-2 py-1 rounded ${isSkipped() ? 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-300' : 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300'}`}>
                {isSkipped() ? 'Skipped' : 'Decided'}
              </span>
            </Show>
          </div>

          {/* Consolidate resolution */}
          <Show when={isConsolidate()}>
            <div class="mt-3">
              <p class="text-sm text-gray-600 dark:text-gray-400">
                Same label — {(() => {
                  const res = group.resolution_type;
                  if (typeof res === 'object' && 'Consolidate' in res) {
                    return res.Consolidate.remove_filter_ids.length;
                  }
                  return 0;
                })()} redundant filter{(() => {
                  const res = group.resolution_type;
                  if (typeof res === 'object' && 'Consolidate' in res) {
                    return res.Consolidate.remove_filter_ids.length !== 1 ? 's' : '';
                  }
                  return 's';
                })()}
              </p>
              <Show when={!isDecided()}>
                <div class="flex gap-2 mt-3">
                  <button
                    data-testid={`accept-${group.group_id}`}
                    onClick={() => handleAcceptConsolidate(group)}
                    class="btn-primary text-sm"
                  >
                    Accept
                  </button>
                  <button
                    data-testid={`skip-${group.group_id}`}
                    onClick={() => handleSkip(group.group_id)}
                    class="btn-secondary text-sm"
                  >
                    Skip
                  </button>
                </div>
              </Show>
            </div>
          </Show>
        </div>
      );
    }}
  </For>
</div>
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 7 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement consolidate card in remediation decide phase"
```

---

### Task 6: MechanicalFix Card

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing test**

```typescript
const mechanicalFixGroup = {
  group_id: 'mechfix-1',
  from_pattern: 'store.com',
  filters: [
    { id: 'f3', from: 'store.com', query: null, subject: null, add_label_ids: ['L2'], remove_label_ids: [] },
    { id: 'f4', from: 'store.com', query: null, subject: 'receipt', add_label_ids: ['L3'], remove_label_ids: [] },
  ],
  label_names: ['Shopping', 'Receipts'],
  resolution_type: {
    MechanicalFix: {
      proposed_replacements: [
        {
          id: null, name: 'store.com', from_pattern: 'store.com',
          is_specific_sender: false, excluded_senders: [],
          subject_keywords: [], excluded_subject_patterns: ['receipt'],
          target_label_id: 'L2', should_archive: false,
        },
        {
          id: null, name: 'store.com-receipt', from_pattern: 'store.com',
          is_specific_sender: false, excluded_senders: [],
          subject_keywords: ['receipt'], excluded_subject_patterns: [],
          target_label_id: 'L3', should_archive: false,
        },
      ],
    },
  },
};

it('renders mechanical fix card with accept fix and skip buttons', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [mechanicalFixGroup] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const card = await page.$('[data-testid="group-card-mechfix-1"]');
    assert.ok(card, 'MechanicalFix card should be rendered');
    const acceptBtn = await page.$('[data-testid="accept-mechfix-1"]');
    assert.ok(acceptBtn, 'Accept Fix button should exist');
    const text = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.textContent);
    assert.ok(text?.includes('Auto-fixable'), 'Should indicate auto-fixable');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npm test`
Expected: FAIL — card exists but no auto-fixable text or accept button for mechanical fix.

**Step 3: Add MechanicalFix rendering**

In `RemediationView.tsx`, inside the `For` callback, after the Consolidate `<Show>` block, add:

```tsx
{/* MechanicalFix resolution */}
<Show when={typeof group.resolution_type === 'object' && 'MechanicalFix' in group.resolution_type}>
  <div class="mt-3">
    <p class="text-sm font-medium text-gray-600 dark:text-gray-400">
      Auto-fixable — add subject exclusions
    </p>
    <div class="mt-2 space-y-1">
      <For each={(() => {
        const res = group.resolution_type;
        if (typeof res === 'object' && 'MechanicalFix' in res) {
          return res.MechanicalFix.proposed_replacements;
        }
        return [];
      })()}>
        {(replacement) => (
          <div class="text-xs font-mono bg-gray-50 dark:bg-gray-700 p-2 rounded">
            {replacement.from_pattern || '(any)'} → {replacement.name}
            <Show when={replacement.excluded_subject_patterns.length > 0}>
              <span class="text-gray-400"> (excl: {replacement.excluded_subject_patterns.join(', ')})</span>
            </Show>
          </div>
        )}
      </For>
    </div>
    <Show when={!isDecided()}>
      <div class="flex gap-2 mt-3">
        <button
          data-testid={`accept-${group.group_id}`}
          onClick={async () => {
            const res = group.resolution_type;
            if (typeof res === 'object' && 'MechanicalFix' in res) {
              const decision: GroupDecision = {
                ReplaceWithExclusive: {
                  replacement_filters: res.MechanicalFix.proposed_replacements,
                },
              };
              remediation.setDecision(group.group_id, decision);
              await api.submitGroupDecision(group.group_id, decision);
            }
          }}
          class="btn-primary text-sm"
        >
          Accept Fix
        </button>
        <button
          data-testid={`skip-${group.group_id}`}
          onClick={() => handleSkip(group.group_id)}
          class="btn-secondary text-sm"
        >
          Skip
        </button>
      </div>
    </Show>
  </div>
</Show>
```

Note: the `handleSkip` function needs to be accessible here. It should be defined inside the `For` callback (which it already is from Task 5).

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 8 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement mechanical fix card in remediation decide phase"
```

---

### Task 7: PickWinner Card

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing test**

```typescript
const pickWinnerGroup = {
  group_id: 'pick-1',
  from_pattern: 'news.com',
  filters: [
    { id: 'f5', from: 'news.com', query: null, subject: null, add_label_ids: ['L4'], remove_label_ids: [] },
    { id: 'f6', from: 'news.com', query: null, subject: 'digest', add_label_ids: ['L5'], remove_label_ids: [] },
  ],
  label_names: ['News', 'Digests'],
  resolution_type: 'PickWinner' as const,
};

it('renders pick winner card with clickable filter rows', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [pickWinnerGroup] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const row1 = await page.$('[data-testid="filter-row-f5"]');
    const row2 = await page.$('[data-testid="filter-row-f6"]');
    assert.ok(row1, 'Filter row f5 should exist');
    assert.ok(row2, 'Filter row f6 should exist');
  } finally {
    await page.close();
  }
});

it('highlights selected winner and marks card as decided', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: [pickWinnerGroup] }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="filter-row-f5"]');
    const classes = await page.$eval('[data-testid="filter-row-f5"]', el => el.className);
    assert.ok(classes.includes('ring-2') || classes.includes('border-primary') || classes.includes('bg-primary'), 'Selected row should be visually highlighted');
    const cardClasses = await page.$eval('[data-testid="group-card-pick-1"]', el => el.className);
    assert.ok(cardClasses.includes('border-l-green'), 'Card should show decided state');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run tests to verify they fail**

Run: `cd ui && npm test`
Expected: FAIL — `filter-row-f5` not found.

**Step 3: Add PickWinner rendering**

In `RemediationView.tsx`, inside the `For` callback, after the MechanicalFix `<Show>` block, add:

```tsx
{/* PickWinner resolution */}
<Show when={group.resolution_type === 'PickWinner'}>
  <div class="mt-3">
    <p class="text-sm font-medium text-gray-600 dark:text-gray-400 mb-2">
      Different labels — pick which filter to keep
    </p>
    <div class="space-y-2">
      <For each={group.filters}>
        {(filter) => {
          const isSelected = createMemo(() => {
            const d = decision();
            return d !== undefined && typeof d === 'object' && 'KeepOne' in d && d.KeepOne.keep_filter_id === filter.id;
          });

          return (
            <button
              data-testid={`filter-row-${filter.id}`}
              onClick={async () => {
                const d: GroupDecision = { KeepOne: { keep_filter_id: filter.id } };
                remediation.setDecision(group.group_id, d);
                await api.submitGroupDecision(group.group_id, d);
              }}
              class={`w-full text-left p-3 rounded-lg border-2 transition-all ${
                isSelected()
                  ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30 ring-2 ring-primary-300'
                  : 'border-gray-200 dark:border-gray-600 hover:border-gray-300 dark:hover:border-gray-500'
              }`}
            >
              <div class="flex items-center justify-between">
                <div>
                  <span class="text-sm font-mono text-gray-900 dark:text-white">
                    {filter.from || filter.query || '(no query)'}
                  </span>
                  <Show when={filter.subject}>
                    <span class="text-xs text-gray-500 dark:text-gray-400 ml-2">
                      subject:{filter.subject}
                    </span>
                  </Show>
                </div>
                <span class="text-xs text-gray-500 dark:text-gray-400">
                  {filter.add_label_ids.join(', ')}
                </span>
              </div>
            </button>
          );
        }}
      </For>
    </div>
    <Show when={!isDecided()}>
      <div class="flex gap-2 mt-3">
        <button
          data-testid={`skip-${group.group_id}`}
          onClick={() => handleSkip(group.group_id)}
          class="btn-secondary text-sm"
        >
          Skip
        </button>
      </div>
    </Show>
  </div>
</Show>
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 10 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement pick winner card with click-to-select"
```

---

### Task 8: Sticky Footer + Review Plan Button

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing test**

```typescript
it('enables Review Plan button only when all groups are decided', async () => {
  const groups = [consolidateGroup, pickWinnerGroup];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({ detect_overlaps: groups }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');

    // Button should be disabled initially
    const disabledAttr = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
    assert.strictEqual(disabledAttr, true, 'Review Plan should be disabled with undecided groups');

    // Decide on first group
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');

    // Still disabled (one group undecided)
    const stillDisabled = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
    assert.strictEqual(stillDisabled, true, 'Should still be disabled with 1 undecided');

    // Decide on second group
    await clickAndWait(page, '[data-testid="filter-row-f5"]');

    // Now enabled
    const nowEnabled = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
    assert.strictEqual(nowEnabled, false, 'Should be enabled when all decided');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npm test`
Expected: FAIL — `review-plan-btn` not found.

**Step 3: Add sticky footer to deciding phase**

In `RemediationView.tsx`, inside the deciding phase `<Show>` block, after the `<div class="space-y-4">...</div>` that contains the cards, add:

```tsx
{/* Sticky footer */}
<div class="sticky bottom-0 bg-white dark:bg-gray-800 border-t border-gray-200 dark:border-gray-700 p-4 -mx-6 mt-6 flex items-center justify-between">
  <span class="text-sm text-gray-600 dark:text-gray-400">
    {remediation.decidedCount()}/{remediation.groups().length} decided
  </span>
  <button
    data-testid="review-plan-btn"
    disabled={!remediation.allDecided()}
    onClick={handleReviewPlan}
    class="btn-primary"
    classList={{ 'opacity-50 cursor-not-allowed': !remediation.allDecided() }}
  >
    Review Plan
  </button>
</div>
```

Add the `handleReviewPlan` function:

```typescript
const handleReviewPlan = async () => {
  try {
    const summary = await api.remediationSummary();
    remediation.setSummary(summary);
    remediation.setPhase('confirming');
  } catch (e) {
    remediation.setError(e instanceof Error ? e.message : String(e));
  }
};
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 11 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: add sticky footer with Review Plan button gated on all-decided"
```

---

### Task 9: Confirm Phase

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing test**

```typescript
it('shows summary and execute button in confirm phase', async () => {
  const summaryText = 'Remediation plan: 1 groups, 1 deletions, 0 creations, 0 skipped\n  consolidate-1 — consolidate (same label), delete 1 redundant filters';
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: [consolidateGroup],
      remediation_summary: summaryText,
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="review-plan-btn"]');

    const summary = await page.$eval('[data-testid="plan-summary"]', el => el.textContent);
    assert.ok(summary?.includes('1 deletions'), 'Should show summary text');

    const executeBtn = await page.$('[data-testid="execute-btn"]');
    assert.ok(executeBtn, 'Execute button should exist');

    const backBtn = await page.$('[data-testid="back-to-decisions-btn"]');
    assert.ok(backBtn, 'Back to Decisions button should exist');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run test to verify it fails**

Run: `cd ui && npm test`
Expected: FAIL — `plan-summary` not found.

**Step 3: Add confirm phase rendering**

In `RemediationView.tsx`, after the deciding `<Show>` block:

```tsx
{/* Phase: Confirming */}
<Show when={remediation.phase() === 'confirming'}>
  <div class="card p-6">
    <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
      Remediation Plan
    </h3>
    <pre data-testid="plan-summary" class="text-sm font-mono bg-gray-50 dark:bg-gray-700 p-4 rounded-lg whitespace-pre-wrap text-gray-700 dark:text-gray-300">
      {remediation.summary()}
    </pre>
    <div class="flex gap-3 mt-6">
      <button
        data-testid="execute-btn"
        onClick={handleExecute}
        class="btn-primary"
      >
        Execute
      </button>
      <button
        data-testid="back-to-decisions-btn"
        onClick={() => remediation.setPhase('deciding')}
        class="btn-secondary"
      >
        Back to Decisions
      </button>
    </div>
  </div>
</Show>
```

Add the `handleExecute` function:

```typescript
const handleExecute = async () => {
  remediation.setPhase('executing');
  try {
    const result = await api.executeRemediation();
    remediation.setResult(result);
    const swaps = await api.collectRemediationSwaps();
    remediation.setSwaps(swaps);
    remediation.setPhase('results');
  } catch (e) {
    remediation.setError(e instanceof Error ? e.message : String(e));
    remediation.setPhase('confirming');
  }
};
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 12 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement confirm phase with summary and execute button"
```

---

### Task 10: Results Phase + Apply

**Files:**
- Modify: `ui/src/components/views/RemediationView.tsx`
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Write failing tests**

```typescript
it('shows execution results with apply button when swaps exist', async () => {
  const swaps = [
    { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
  ];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: [consolidateGroup],
      remediation_summary: 'plan',
      execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: swaps,
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="review-plan-btn"]');
    await clickAndWait(page, '[data-testid="execute-btn"]');

    const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
    assert.ok(deleted?.includes('1'), 'Should show 1 deleted');

    const applyBtn = await page.$('[data-testid="apply-swaps-btn"]');
    assert.ok(applyBtn, 'Apply Label Changes button should exist');
  } finally {
    await page.close();
  }
});

it('shows relabeled count after applying swaps', async () => {
  const swaps = [
    { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
  ];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: [consolidateGroup],
      remediation_summary: 'plan',
      execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: swaps,
      apply_remediation_swaps: { messages_relabeled: 42, messages_failed: 0, errors: [] },
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="review-plan-btn"]');
    await clickAndWait(page, '[data-testid="execute-btn"]');
    await clickAndWait(page, '[data-testid="apply-swaps-btn"]');

    const relabeled = await page.$eval('[data-testid="apply-relabeled"]', el => el.textContent);
    assert.ok(relabeled?.includes('42'), 'Should show 42 messages relabeled');
  } finally {
    await page.close();
  }
});

it('shows no label changes needed when no swaps', async () => {
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: [consolidateGroup],
      remediation_summary: 'plan',
      execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: [],
    }));
    await clickAndWait(page, '[data-testid="nav-remediation"]');
    await clickAndWait(page, '[data-testid="detect-btn"]');
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="review-plan-btn"]');
    await clickAndWait(page, '[data-testid="execute-btn"]');

    const msg = await page.$eval('[data-testid="no-swaps"]', el => el.textContent);
    assert.ok(msg?.includes('No label changes'), 'Should show no swaps message');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run tests to verify they fail**

Run: `cd ui && npm test`
Expected: FAIL — `result-deleted` not found.

**Step 3: Add results phase rendering**

In `RemediationView.tsx`, add executing spinner and results rendering:

```tsx
{/* Phase: Executing */}
<Show when={remediation.phase() === 'executing'}>
  <div class="card p-8 text-center">
    <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
    <p class="text-gray-500 dark:text-gray-400">Executing remediation plan...</p>
  </div>
</Show>

{/* Phase: Results */}
<Show when={remediation.phase() === 'results'}>
  <div class="space-y-4">
    {/* Execution results */}
    <div class="card p-6">
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
        Execution Results
      </h3>
      <div class="grid grid-cols-3 gap-4">
        <div class="text-center p-4 bg-red-50 dark:bg-red-900/30 rounded-lg">
          <div data-testid="result-deleted" class="text-2xl font-bold text-red-600 dark:text-red-400">
            {remediation.result()?.deleted.length ?? 0}
          </div>
          <div class="text-sm text-gray-500 dark:text-gray-400">Deleted</div>
        </div>
        <div class="text-center p-4 bg-green-50 dark:bg-green-900/30 rounded-lg">
          <div data-testid="result-created" class="text-2xl font-bold text-green-600 dark:text-green-400">
            {remediation.result()?.created.length ?? 0}
          </div>
          <div class="text-sm text-gray-500 dark:text-gray-400">Created</div>
        </div>
        <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
          <div data-testid="result-skipped" class="text-2xl font-bold text-gray-400">
            {remediation.result()?.skipped ?? 0}
          </div>
          <div class="text-sm text-gray-500 dark:text-gray-400">Skipped</div>
        </div>
      </div>

      <Show when={(remediation.result()?.errors.length ?? 0) > 0}>
        <div class="mt-4 p-3 bg-red-50 dark:bg-red-900/30 rounded-lg">
          <p class="text-sm font-medium text-red-700 dark:text-red-300 mb-2">Errors:</p>
          <ul class="text-sm text-red-600 dark:text-red-400 space-y-1">
            <For each={remediation.result()?.errors ?? []}>
              {(err) => <li>- {err}</li>}
            </For>
          </ul>
        </div>
      </Show>
    </div>

    {/* Label swaps */}
    <Show when={remediation.swaps().length > 0 && !remediation.applyResult()}>
      <div class="card p-6">
        <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
          Label Changes for Existing Emails
        </h3>
        <div class="space-y-2 mb-4">
          <For each={remediation.swaps()}>
            {(swap) => (
              <div class="text-sm font-mono bg-gray-50 dark:bg-gray-700 p-2 rounded">
                {swap.query} — remove {swap.remove_label_ids.join(', ')} → add {swap.add_label_id}
              </div>
            )}
          </For>
        </div>
        <button
          data-testid="apply-swaps-btn"
          onClick={handleApplySwaps}
          class="btn-primary"
        >
          Apply Label Changes
        </button>
      </div>
    </Show>

    <Show when={remediation.swaps().length === 0 && !remediation.applyResult()}>
      <div data-testid="no-swaps" class="card p-6 text-center">
        <p class="text-gray-500 dark:text-gray-400">
          No label changes needed for existing emails.
        </p>
      </div>
    </Show>

    {/* Apply result */}
    <Show when={remediation.applyResult()}>
      <div class="card p-6">
        <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
          Label Application Results
        </h3>
        <div data-testid="apply-relabeled" class="text-sm text-gray-700 dark:text-gray-300">
          {remediation.applyResult()!.messages_relabeled} messages relabeled
        </div>
        <Show when={remediation.applyResult()!.messages_failed > 0}>
          <div class="text-sm text-red-600 dark:text-red-400 mt-1">
            {remediation.applyResult()!.messages_failed} messages failed
          </div>
        </Show>
      </div>
    </Show>

    {/* Done button */}
    <div class="flex justify-center">
      <button
        data-testid="done-btn"
        onClick={() => {
          remediation.reset();
          setHasDetected(false);
        }}
        class="btn-secondary"
      >
        Done
      </button>
    </div>
  </div>
</Show>
```

Add the `handleApplySwaps` function:

```typescript
const handleApplySwaps = async () => {
  try {
    const result = await api.applyRemediationSwaps();
    remediation.setApplyResult(result);
  } catch (e) {
    remediation.setError(e instanceof Error ? e.message : String(e));
  }
};
```

**Step 4: Run tests to verify they pass**

Run: `cd ui && npm test`
Expected: All 15 tests pass.

**Step 5: Commit**

```bash
git add ui/src/components/views/RemediationView.tsx ui/tests/remediation.test.ts
git commit -m "feat: implement results phase with execution stats and label swap apply"
```

---

### Task 11: Final Integration Test + Cleanup

**Files:**
- Modify: `ui/tests/remediation.test.ts`

**Step 1: Add the full end-to-end test**

```typescript
it('completes full flow: detect -> decide -> confirm -> execute -> apply -> done', async () => {
  const groups = [consolidateGroup, pickWinnerGroup];
  const page = await newPage();
  try {
    await navigateWithMock(page, baseMock({
      detect_overlaps: groups,
      remediation_summary: 'Remediation plan: 2 groups, 2 deletions, 0 creations, 0 skipped',
      execute_remediation: { deleted: ['f2', 'f6'], created: [], skipped: 0, errors: [] },
      collect_remediation_swaps: [
        { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
      ],
      apply_remediation_swaps: { messages_relabeled: 10, messages_failed: 0, errors: [] },
    }));

    // Navigate
    await clickAndWait(page, '[data-testid="nav-remediation"]');

    // Detect
    await clickAndWait(page, '[data-testid="detect-btn"]');
    const header = await page.$eval('[data-testid="decide-header"]', el => el.textContent);
    assert.ok(header?.includes('2'), 'Should show 2 groups');

    // Decide
    await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
    await clickAndWait(page, '[data-testid="filter-row-f5"]');

    // Review
    await clickAndWait(page, '[data-testid="review-plan-btn"]');
    const summary = await page.$('[data-testid="plan-summary"]');
    assert.ok(summary, 'Should show plan summary');

    // Execute
    await clickAndWait(page, '[data-testid="execute-btn"]');
    const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
    assert.ok(deleted?.includes('2'), 'Should show 2 deleted');

    // Apply
    await clickAndWait(page, '[data-testid="apply-swaps-btn"]');
    const relabeled = await page.$eval('[data-testid="apply-relabeled"]', el => el.textContent);
    assert.ok(relabeled?.includes('10'), 'Should show 10 relabeled');

    // Done
    await clickAndWait(page, '[data-testid="done-btn"]');
    const detectBtn = await page.$('[data-testid="detect-btn"]');
    assert.ok(detectBtn, 'Should be back at idle phase with detect button');
  } finally {
    await page.close();
  }
});
```

**Step 2: Run all tests**

Run: `cd ui && npm test`
Expected: All 16 tests pass.

**Step 3: Run TypeScript typecheck**

Run: `cd ui && npm run typecheck`
Expected: No errors.

**Step 4: Commit**

```bash
git add ui/tests/remediation.test.ts
git commit -m "test: add full end-to-end integration test for remediation flow"
```
