import { Component, createSignal, onMount, Show, For } from 'solid-js';
import { filters } from '../../stores/app';
import * as api from '../../lib/api';
import type { FilterView, AnalysisView } from '../../types';

const FiltersView: Component = () => {
  const [isLoading, setIsLoading] = createSignal(true);
  const [analysis, setAnalysis] = createSignal<AnalysisView | null>(null);
  const [applyResult, setApplyResult] = createSignal<{ success: boolean; message: string } | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  onMount(async () => {
    await loadFilters();
  });

  const loadFilters = async () => {
    setIsLoading(true);
    setError(null);

    try {
      // Fetch existing filters
      await api.getExistingFilters();

      // Generate proposed filters from decisions
      await api.generateProposedFilters();

      // Compare filters
      const comparison = await api.compareFilters();
      filters.setComparison(comparison);

      // Analyze overlaps
      const analysisResult = await api.analyzeFilterOverlaps(false);
      setAnalysis(analysisResult);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const handleApply = async (dryRun: boolean) => {
    setApplyResult(null);
    setError(null);

    try {
      const result = await api.applyFilters(dryRun);

      if (result.success) {
        setApplyResult({
          success: true,
          message: dryRun
            ? `Dry run complete: Would create ${result.created} filters`
            : `Successfully created ${result.created} filters`,
        });
      } else {
        setApplyResult({
          success: false,
          message: `Created ${result.created} filters, ${result.failed} failed`,
        });
        if (result.errors.length > 0) {
          setError(result.errors.join('\n'));
        }
      }

      if (!dryRun) {
        await loadFilters();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const getChangeIcon = (type: string) => {
    switch (type) {
      case 'new':
        return { icon: '+', class: 'text-green-600 dark:text-green-400' };
      case 'modified':
        return { icon: '~', class: 'text-yellow-600 dark:text-yellow-400' };
      case 'deleted':
        return { icon: '-', class: 'text-red-600 dark:text-red-400' };
      default:
        return { icon: '=', class: 'text-gray-400' };
    }
  };

  return (
    <div class="space-y-6">
      {/* Header */}
      <div class="flex items-center justify-between">
        <div>
          <h2 class="text-xl font-bold text-gray-900 dark:text-white">
            Filter Visualization
          </h2>
          <p class="text-gray-500 dark:text-gray-400">
            Compare current and proposed filters
          </p>
        </div>

        <div class="flex gap-2">
          <button
            onClick={() => handleApply(true)}
            disabled={isLoading()}
            class="btn-secondary"
          >
            Dry Run
          </button>
          <button
            onClick={() => handleApply(false)}
            disabled={isLoading()}
            class="btn-primary"
          >
            Apply Filters
          </button>
        </div>
      </div>

      {/* Loading */}
      <Show when={isLoading()}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Loading filters...</p>
        </div>
      </Show>

      {/* Error */}
      <Show when={error()}>
        <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{error()}</p>
        </div>
      </Show>

      {/* Apply Result */}
      <Show when={applyResult()}>
        <div
          class="card p-4"
          classList={{
            'border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30': applyResult()!.success,
            'border-yellow-200 dark:border-yellow-800 bg-yellow-50 dark:bg-yellow-900/30': !applyResult()!.success,
          }}
        >
          <p
            classList={{
              'text-green-700 dark:text-green-300': applyResult()!.success,
              'text-yellow-700 dark:text-yellow-300': !applyResult()!.success,
            }}
          >
            {applyResult()!.message}
          </p>
        </div>
      </Show>

      {/* Summary */}
      <Show when={filters.summary()}>
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Change Summary
          </h3>

          <div class="grid grid-cols-5 gap-4">
            <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div class="text-2xl font-bold text-gray-400">
                {filters.summary()!.unchanged}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Unchanged</div>
            </div>
            <div class="text-center p-4 bg-green-50 dark:bg-green-900/30 rounded-lg">
              <div class="text-2xl font-bold text-green-600 dark:text-green-400">
                {filters.summary()!.new}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">New</div>
            </div>
            <div class="text-center p-4 bg-yellow-50 dark:bg-yellow-900/30 rounded-lg">
              <div class="text-2xl font-bold text-yellow-600 dark:text-yellow-400">
                {filters.summary()!.modified}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Modified</div>
            </div>
            <div class="text-center p-4 bg-red-50 dark:bg-red-900/30 rounded-lg">
              <div class="text-2xl font-bold text-red-600 dark:text-red-400">
                {filters.summary()!.deleted}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Deleted</div>
            </div>
            <div class="text-center p-4 bg-primary-50 dark:bg-primary-900/30 rounded-lg">
              <div class="text-2xl font-bold text-primary-600 dark:text-primary-400">
                {filters.summary()!.total_affected}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Emails Affected</div>
            </div>
          </div>
        </div>
      </Show>

      {/* Overlap Analysis */}
      <Show when={analysis() && (analysis()!.error_count > 0 || analysis()!.warning_count > 0)}>
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Overlap Analysis
          </h3>

          <p class="text-sm text-gray-500 dark:text-gray-400 mb-4">
            {analysis()!.summary}
          </p>

          <div class="space-y-3">
            <For each={analysis()!.conflicts.filter(c => c.severity !== 'Info')}>
              {(conflict) => (
                <div
                  class="p-4 rounded-lg border"
                  classList={{
                    'bg-red-50 dark:bg-red-900/30 border-red-200 dark:border-red-800': conflict.severity === 'Error',
                    'bg-yellow-50 dark:bg-yellow-900/30 border-yellow-200 dark:border-yellow-800': conflict.severity === 'Warning',
                  }}
                >
                  <div class="flex items-start gap-3">
                    <span
                      class="text-sm font-medium px-2 py-0.5 rounded flex-shrink-0"
                      classList={{
                        'bg-red-100 text-red-700 dark:bg-red-800 dark:text-red-200': conflict.severity === 'Error',
                        'bg-yellow-100 text-yellow-700 dark:bg-yellow-800 dark:text-yellow-200': conflict.severity === 'Warning',
                      }}
                    >
                      {conflict.severity}
                    </span>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-gray-900 dark:text-white">
                        {conflict.conflict_type}
                      </p>
                      <p class="text-sm text-gray-600 dark:text-gray-400 mt-1">
                        {conflict.description}
                      </p>

                      {/* Filter details */}
                      <div class="mt-3 space-y-2 text-xs">
                        <div class="p-2 bg-white dark:bg-gray-800 rounded border border-gray-200 dark:border-gray-600">
                          <p class="font-medium text-gray-700 dark:text-gray-300 mb-1">Filter A:</p>
                          <p class="font-mono text-gray-600 dark:text-gray-400 break-all">
                            {conflict.filter_a_query || conflict.filter_a_name}
                          </p>
                          <Show when={conflict.filter_a_label}>
                            <p class="text-gray-500 dark:text-gray-500 mt-1">
                              Label: <span class="font-medium">{conflict.filter_a_label}</span>
                            </p>
                          </Show>
                        </div>
                        <div class="p-2 bg-white dark:bg-gray-800 rounded border border-gray-200 dark:border-gray-600">
                          <p class="font-medium text-gray-700 dark:text-gray-300 mb-1">Filter B:</p>
                          <p class="font-mono text-gray-600 dark:text-gray-400 break-all">
                            {conflict.filter_b_query || conflict.filter_b_name}
                          </p>
                          <Show when={conflict.filter_b_label}>
                            <p class="text-gray-500 dark:text-gray-500 mt-1">
                              Label: <span class="font-medium">{conflict.filter_b_label}</span>
                            </p>
                          </Show>
                        </div>
                      </div>

                      <Show when={conflict.suggestions.length > 0}>
                        <div class="mt-3">
                          <p class="text-xs text-gray-500 dark:text-gray-500">Suggestions:</p>
                          <ul class="text-xs text-gray-600 dark:text-gray-400 mt-1 space-y-1">
                            <For each={conflict.suggestions}>
                              {(suggestion) => <li>• {suggestion}</li>}
                            </For>
                          </ul>
                        </div>
                      </Show>
                    </div>
                  </div>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* Filter Lists */}
      <div class="grid grid-cols-2 gap-6">
        {/* Existing Filters */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Existing Filters ({filters.existingFilters().length})
          </h3>

          <div class="space-y-2 max-h-96 overflow-auto">
            <For each={filters.existingFilters()}>
              {(filter) => {
                const change = getChangeIcon(filter.change_type);
                return (
                  <div class="p-3 bg-gray-50 dark:bg-gray-700 rounded-lg">
                    <div class="flex items-center gap-2">
                      <span class={`font-mono ${change.class}`}>[{change.icon}]</span>
                      <span class="text-sm font-mono text-gray-900 dark:text-white truncate">
                        {filter.query || filter.name}
                      </span>
                    </div>
                    <div class="flex gap-4 mt-1 text-xs text-gray-500 dark:text-gray-400">
                      <span>Label: {filter.label || 'None'}</span>
                      <span>Archive: {filter.archive ? 'Yes' : 'No'}</span>
                    </div>
                  </div>
                );
              }}
            </For>

            <Show when={filters.existingFilters().length === 0}>
              <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
                No existing filters found
              </p>
            </Show>
          </div>
        </div>

        {/* Proposed Filters */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Proposed Filters ({filters.proposedFilters().length})
          </h3>

          <div class="space-y-2 max-h-96 overflow-auto">
            <For each={filters.proposedFilters()}>
              {(filter) => {
                const change = getChangeIcon(filter.change_type);
                return (
                  <div class="p-3 bg-gray-50 dark:bg-gray-700 rounded-lg">
                    <div class="flex items-center gap-2">
                      <span class={`font-mono ${change.class}`}>[{change.icon}]</span>
                      <span class="text-sm font-mono text-gray-900 dark:text-white truncate">
                        {filter.query}
                      </span>
                    </div>
                    <div class="flex gap-4 mt-1 text-xs text-gray-500 dark:text-gray-400">
                      <span>Label: {filter.label}</span>
                      <span>Archive: {filter.archive ? 'Yes' : 'No'}</span>
                      <span>{filter.estimated_matches} emails</span>
                    </div>
                  </div>
                );
              }}
            </For>

            <Show when={filters.proposedFilters().length === 0}>
              <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
                No proposed filters. Review clusters first.
              </p>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
};

export default FiltersView;
