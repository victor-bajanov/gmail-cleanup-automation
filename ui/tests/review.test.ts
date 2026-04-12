import { describe, it, before, after } from 'node:test';
import assert from 'node:assert';
import { setupBrowser, teardownBrowser, newPage, navigateWithMock, clickAndWait } from './setup.ts';
import { buildMockScript } from './mock-tauri.ts';

/**
 * Mock clusters with non-contiguous indices (simulating exclusion filtering).
 * Backend returns indices 0, 2, 3 — index 1 was excluded.
 */
const clustersWithGaps = [
  {
    index: 0,
    sender_pattern: '*@alpha.com',
    email_count: 10,
    suggested_label: 'Alpha',
    should_archive: false,
    sample_subjects: ['Hello from Alpha'],
    sample_senders: ['*@alpha.com'],
    has_existing_filter: false,
    decided: false,
  },
  {
    index: 2,
    sender_pattern: '*@charlie.com',
    email_count: 5,
    suggested_label: 'Charlie',
    should_archive: false,
    sample_subjects: ['Charlie update'],
    sample_senders: ['*@charlie.com'],
    has_existing_filter: true,
    existing_filter_label: 'OldCharlie',
    existing_filter_archive: false,
    decided: false,
  },
  {
    index: 3,
    sender_pattern: '*@delta.com',
    email_count: 8,
    suggested_label: 'Delta',
    should_archive: true,
    sample_subjects: ['Delta newsletter'],
    sample_senders: ['*@delta.com'],
    has_existing_filter: false,
    decided: false,
  },
];

function reviewMock(overrides: Record<string, unknown> = {}): string {
  return buildMockScript({
    'plugin:event|listen': 0,
    check_auth_status: { authenticated: true, email: 'test@example.com', credentials_path: '', credentials_exist: true, token_exists: true },
    initialize_client: true,
    match_existing_filters: { matched_count: 0, new_count: 3, total_count: 3 },
    get_clusters: clustersWithGaps,
    get_next_undecided_cluster: 0,
    get_review_summary: { total: 3, accepted: 0, rejected: 0, skipped: 0, deleted: 0, excluded: 0, remaining: 3, existing_filters: 1, existing_remaining: 1 },
    submit_cluster_decision: true,
    undo_last_decision: null,
    editor_get_filters: { filters: [], label_map: {} },
    ...overrides,
  });
}

describe('Review — cluster selection with index gaps', () => {
  before(async () => { await setupBrowser(); });
  after(async () => { await teardownBrowser(); });

  it('clicking a cluster with non-contiguous index shows the correct detail', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, reviewMock());
      await clickAndWait(page, '[data-testid="nav-review"]');

      // Wait for clusters to load
      await page.waitForSelector('.font-mono', { timeout: 5000 });
      await new Promise(r => setTimeout(r, 500));

      // The left pane is sorted: existing filters first.
      // So charlie.com (has_existing_filter) should appear first in the sorted list.
      // Click on charlie.com in the left pane — its backend index is 2.
      // The detail pane should show charlie.com, NOT delta.com.

      // Find and click the cluster button containing 'charlie.com'
      const clicked = await page.evaluate(() => {
        const buttons = document.querySelectorAll('button');
        for (const btn of buttons) {
          if (btn.textContent?.includes('charlie.com')) {
            btn.click();
            return true;
          }
        }
        return false;
      });
      assert.ok(clicked, 'Should find and click charlie.com button');

      await new Promise(r => setTimeout(r, 500));

      // The detail pane header should show charlie.com
      const detailText = await page.evaluate(() => {
        const h2 = document.querySelector('h2.font-mono');
        return h2?.textContent ?? '';
      });

      assert.ok(
        detailText.includes('charlie.com'),
        `Detail pane should show charlie.com but got: "${detailText}"`
      );
    } finally {
      await page.close();
    }
  });

  it('clicking different clusters always shows the matching detail', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, reviewMock());
      await clickAndWait(page, '[data-testid="nav-review"]');
      await page.waitForSelector('.font-mono', { timeout: 5000 });
      await new Promise(r => setTimeout(r, 500));

      // Click delta.com (index 3, last in unsorted list)
      await page.evaluate(() => {
        const buttons = document.querySelectorAll('button');
        for (const btn of buttons) {
          if (btn.textContent?.includes('delta.com')) {
            btn.click();
            return;
          }
        }
      });
      await new Promise(r => setTimeout(r, 500));

      const deltaDetail = await page.evaluate(() => {
        const h2 = document.querySelector('h2.font-mono');
        return h2?.textContent ?? '';
      });
      assert.ok(
        deltaDetail.includes('delta.com'),
        `Should show delta.com but got: "${deltaDetail}"`
      );

      // Now click alpha.com (index 0)
      await page.evaluate(() => {
        const buttons = document.querySelectorAll('button');
        for (const btn of buttons) {
          if (btn.textContent?.includes('alpha.com')) {
            btn.click();
            return;
          }
        }
      });
      await new Promise(r => setTimeout(r, 500));

      const alphaDetail = await page.evaluate(() => {
        const h2 = document.querySelector('h2.font-mono');
        return h2?.textContent ?? '';
      });
      assert.ok(
        alphaDetail.includes('alpha.com'),
        `Should show alpha.com but got: "${alphaDetail}"`
      );
    } finally {
      await page.close();
    }
  });
});
