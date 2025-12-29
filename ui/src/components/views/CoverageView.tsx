import { Component, createSignal, onMount, Show, For } from 'solid-js';
import { coverage } from '../../stores/app';
import * as api from '../../lib/api';
import type { CoverageAnalysis, OverlapMatrixResult, UncoveredEmail } from '../../types';

const CoverageView: Component = () => {
  const [isLoading, setIsLoading] = createSignal(true);
  const [overlapMatrix, setOverlapMatrix] = createSignal<OverlapMatrixResult | null>(null);
  const [uncoveredEmails, setUncoveredEmails] = createSignal<UncoveredEmail[]>([]);
  const [activeTab, setActiveTab] = createSignal<'overview' | 'breakdown' | 'uncovered'>('overview');
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    await loadCoverage();
  });

  const loadCoverage = async () => {
    setIsLoading(true);
    setError(null);

    try {
      // Get coverage analysis
      const analysis = await api.analyzeCoverage();
      coverage.setAnalysis(analysis);

      // Get overlap matrix
      const matrix = await api.getOverlapMatrix();
      setOverlapMatrix(matrix);

      // Get uncovered emails
      const uncovered = await api.getUncoveredEmails(50);
      setUncoveredEmails(uncovered);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const getCoverageColor = (percentage: number) => {
    if (percentage >= 80) return 'text-green-600 dark:text-green-400';
    if (percentage >= 60) return 'text-yellow-600 dark:text-yellow-400';
    return 'text-red-600 dark:text-red-400';
  };

  const getBarWidth = (count: number, max: number) => {
    if (max === 0) return '0%';
    return `${Math.round((count / max) * 100)}%`;
  };

  return (
    <div class="space-y-6">
      {/* Header */}
      <div class="flex items-center justify-between">
        <div>
          <h2 class="text-xl font-bold text-gray-900 dark:text-white">
            Coverage Analysis
          </h2>
          <p class="text-gray-500 dark:text-gray-400">
            Visualize filter coverage and find gaps
          </p>
        </div>

        <button
          onClick={loadCoverage}
          disabled={isLoading()}
          class="btn-secondary"
        >
          {isLoading() ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {/* Loading */}
      <Show when={isLoading()}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Analyzing coverage...</p>
        </div>
      </Show>

      {/* Error */}
      <Show when={error()}>
        <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{error()}</p>
        </div>
      </Show>

      <Show when={!isLoading() && coverage.analysis()}>
        {/* Coverage Overview */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Coverage Overview
          </h3>

          <div class="flex items-center gap-8">
            {/* Progress Circle */}
            <div class="relative w-32 h-32">
              <svg class="w-full h-full transform -rotate-90" viewBox="0 0 36 36">
                {/* Background circle */}
                <circle
                  cx="18"
                  cy="18"
                  r="16"
                  fill="none"
                  class="stroke-gray-200 dark:stroke-gray-700"
                  stroke-width="3"
                />
                {/* Progress circle */}
                <circle
                  cx="18"
                  cy="18"
                  r="16"
                  fill="none"
                  class="stroke-primary-600 transition-all duration-500"
                  stroke-width="3"
                  stroke-linecap="round"
                  stroke-dasharray={`${coverage.coveragePercentage()}, 100`}
                />
              </svg>
              <div class="absolute inset-0 flex items-center justify-center">
                <span class={`text-2xl font-bold ${getCoverageColor(coverage.coveragePercentage())}`}>
                  {Math.round(coverage.coveragePercentage())}%
                </span>
              </div>
            </div>

            {/* Stats */}
            <div class="flex-1 grid grid-cols-3 gap-4">
              <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
                <div class="text-2xl font-bold text-gray-900 dark:text-white">
                  {coverage.analysis()!.total_emails}
                </div>
                <div class="text-sm text-gray-500 dark:text-gray-400">Total Emails</div>
              </div>
              <div class="text-center p-4 bg-green-50 dark:bg-green-900/30 rounded-lg">
                <div class="text-2xl font-bold text-green-600 dark:text-green-400">
                  {coverage.analysis()!.covered_emails}
                </div>
                <div class="text-sm text-gray-500 dark:text-gray-400">Covered</div>
              </div>
              <div class="text-center p-4 bg-red-50 dark:bg-red-900/30 rounded-lg">
                <div class="text-2xl font-bold text-red-600 dark:text-red-400">
                  {coverage.analysis()!.uncovered_emails}
                </div>
                <div class="text-sm text-gray-500 dark:text-gray-400">Uncovered</div>
              </div>
            </div>
          </div>
        </div>

        {/* Tabs */}
        <div class="border-b border-gray-200 dark:border-gray-700">
          <nav class="flex gap-4">
            <button
              onClick={() => setActiveTab('overview')}
              class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
              classList={{
                'border-primary-600 text-primary-600 dark:text-primary-400': activeTab() === 'overview',
                'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400': activeTab() !== 'overview',
              }}
            >
              Filter Breakdown
            </button>
            <button
              onClick={() => setActiveTab('breakdown')}
              class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
              classList={{
                'border-primary-600 text-primary-600 dark:text-primary-400': activeTab() === 'breakdown',
                'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400': activeTab() !== 'breakdown',
              }}
            >
              Uncovered Domains
            </button>
            <button
              onClick={() => setActiveTab('uncovered')}
              class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
              classList={{
                'border-primary-600 text-primary-600 dark:text-primary-400': activeTab() === 'uncovered',
                'border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400': activeTab() !== 'uncovered',
              }}
            >
              Uncovered Emails
            </button>
          </nav>
        </div>

        {/* Tab Content */}
        <Show when={activeTab() === 'overview'}>
          <div class="card p-6">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
              Filter Breakdown
            </h3>

            <div class="space-y-3">
              <For each={coverage.analysis()!.filter_coverage}>
                {(filter) => {
                  const maxCount = Math.max(...coverage.analysis()!.filter_coverage.map(f => f.email_count));
                  return (
                    <div class="space-y-1">
                      <div class="flex justify-between text-sm">
                        <span class="font-mono text-gray-900 dark:text-white truncate max-w-md">
                          {filter.filter_name}
                        </span>
                        <span class="text-gray-500 dark:text-gray-400">
                          {filter.email_count} ({filter.percentage.toFixed(1)}%)
                        </span>
                      </div>
                      <div class="h-2 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                        <div
                          class="h-full bg-primary-600 rounded-full transition-all duration-300"
                          style={{ width: getBarWidth(filter.email_count, maxCount) }}
                        />
                      </div>
                    </div>
                  );
                }}
              </For>

              <Show when={coverage.analysis()!.filter_coverage.length === 0}>
                <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
                  No filter coverage data. Generate proposed filters first.
                </p>
              </Show>
            </div>
          </div>
        </Show>

        <Show when={activeTab() === 'breakdown'}>
          <div class="card p-6">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
              Top Uncovered Domains
            </h3>

            <div class="space-y-2">
              <For each={coverage.analysis()!.top_uncovered_domains}>
                {(domain) => {
                  const maxCount = coverage.analysis()!.top_uncovered_domains[0]?.count || 1;
                  return (
                    <div class="flex items-center gap-4 py-2 border-b border-gray-100 dark:border-gray-700 last:border-0">
                      <span class="font-mono text-sm text-gray-900 dark:text-white w-48 truncate">
                        {domain.domain}
                      </span>
                      <div class="flex-1 h-2 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                        <div
                          class="h-full bg-red-500 rounded-full"
                          style={{ width: getBarWidth(domain.count, maxCount) }}
                        />
                      </div>
                      <span class="text-sm text-gray-500 dark:text-gray-400 w-20 text-right">
                        {domain.count} emails
                      </span>
                    </div>
                  );
                }}
              </For>

              <Show when={coverage.analysis()!.top_uncovered_domains.length === 0}>
                <p class="text-sm text-green-600 dark:text-green-400 text-center py-4">
                  All domains are covered by filters!
                </p>
              </Show>
            </div>
          </div>
        </Show>

        <Show when={activeTab() === 'uncovered'}>
          <div class="card p-6">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
              Uncovered Emails
            </h3>

            <div class="overflow-auto max-h-96">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-gray-200 dark:border-gray-700">
                    <th class="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Sender</th>
                    <th class="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Subject</th>
                    <th class="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Date</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={uncoveredEmails()}>
                    {(email) => (
                      <tr class="border-b border-gray-100 dark:border-gray-700 last:border-0">
                        <td class="py-2">
                          <div class="font-mono text-gray-900 dark:text-white text-xs truncate max-w-xs">
                            {email.sender}
                          </div>
                        </td>
                        <td class="py-2">
                          <div class="text-gray-700 dark:text-gray-300 truncate max-w-md">
                            {email.subject}
                          </div>
                        </td>
                        <td class="py-2 text-gray-500 dark:text-gray-400">
                          {new Date(email.date).toLocaleDateString()}
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>

              <Show when={uncoveredEmails().length === 0}>
                <p class="text-sm text-green-600 dark:text-green-400 text-center py-4">
                  All emails are covered by filters!
                </p>
              </Show>
            </div>
          </div>
        </Show>
      </Show>
    </div>
  );
};

export default CoverageView;
