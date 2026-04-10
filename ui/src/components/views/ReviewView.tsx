import { Component, createSignal, createEffect, createMemo, Show, For, onMount, onCleanup } from 'solid-js';
import { review, navigation } from '../../stores/app';
import * as api from '../../lib/api';
import type { ClusterView, DecisionInput } from '../../types';

const ReviewView: Component = () => {
  const [selectedIndex, setSelectedIndex] = createSignal<number | null>(null);
  const [customLabel, setCustomLabel] = createSignal('');
  const [showCustomLabel, setShowCustomLabel] = createSignal(false);
  const [isSubmitting, setIsSubmitting] = createSignal(false);
  const [categories, setCategories] = createSignal<string[]>([]);

  // Sort clusters with existing filters first
  const sortedClusters = createMemo(() => {
    const clusters = [...review.clusters()];
    return clusters.sort((a, b) => {
      // Existing filters first
      if (a.has_existing_filter && !b.has_existing_filter) return -1;
      if (!a.has_existing_filter && b.has_existing_filter) return 1;
      // Then by original index
      return a.index - b.index;
    });
  });

  // Load clusters and select first undecided
  onMount(async () => {
    // Match existing filters to clusters first
    // This populates has_existing_filter, existing_filter_label, etc.
    try {
      const matchResult = await api.matchExistingFilters();
      console.log(`Matched ${matchResult.matched_count} existing filters, ${matchResult.new_count} new`);
    } catch (e) {
      console.warn('Failed to match existing filters:', e);
    }

    const clusters = await api.getClusters();
    review.setClusters(clusters);

    const nextIndex = await api.getNextUndecidedCluster();
    if (nextIndex !== null) {
      setSelectedIndex(nextIndex);
    } else if (clusters.length > 0) {
      setSelectedIndex(0);
    }

    const summary = await api.getReviewSummary();
    review.setSummary(summary);

    try {
      const editorData = await api.editorGetFilters(false);
      const labelNames = Object.values(editorData.label_map);
      const cats = new Set<string>();
      for (const name of labelNames) {
        const parts = name.split('/');
        if (parts.length >= 2) {
          cats.add(parts[1]);
        }
      }
      setCategories([...cats].sort());
    } catch (e) {
      console.warn('Failed to load label categories:', e);
    }
  });

  // Keyboard shortcuts
  onMount(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if (showCustomLabel()) return; // Don't handle shortcuts when typing

      const cluster = currentCluster();

      // Shift+S: Skip all existing filters (works without a cluster selected)
      if (e.key.toLowerCase() === 's' && e.shiftKey) {
        e.preventDefault();
        await handleSkipAllExisting();
        return;
      }

      if (!cluster) return;

      switch (e.key.toLowerCase()) {
        case 'y':
        case 'enter':
          await handleDecision('accept');
          break;
        case 'n':
          await handleDecision('reject');
          break;
        case 's':
          await handleDecision('skip');
          break;
        case 'd':
          await handleDecision('delete');
          break;
        case 'e':
          await handleDecision('exclude');
          break;
        case 'l':
          setCustomLabel(currentCluster()?.suggested_label || '');
          setShowCustomLabel(true);
          break;
        case 'a': {
          const idx = selectedIndex();
          if (idx === null) break;
          const updated = review.clusters().map((c, i) =>
            i === idx ? { ...c, should_archive: !c.should_archive } : c
          );
          review.setClusters(updated);
          break;
        }
        case 'u':
          await handleUndo();
          break;
        case 'arrowup':
          navigateCluster(-1);
          break;
        case 'arrowdown':
          navigateCluster(1);
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    onCleanup(() => window.removeEventListener('keydown', handleKeyDown));
  });

  const currentCluster = () => {
    const idx = selectedIndex();
    if (idx === null) return null;
    return review.clusters()[idx] || null;
  };

  const navigateCluster = (delta: number) => {
    const idx = selectedIndex();
    if (idx === null) return;
    const newIdx = Math.max(0, Math.min(review.clusters().length - 1, idx + delta));
    setSelectedIndex(newIdx);
  };

  const handleDecision = async (action: string, label?: string) => {
    const cluster = currentCluster();
    if (!cluster || isSubmitting()) return;

    setIsSubmitting(true);

    try {
      const input: DecisionInput = {
        cluster_index: cluster.index,
        action,
        custom_label: label || (action === 'customlabel' ? customLabel() : undefined),
        archive: cluster.should_archive,
      };

      await api.submitClusterDecision(input);

      // Refresh clusters and summary
      const clusters = await api.getClusters();
      review.setClusters(clusters);

      const summary = await api.getReviewSummary();
      review.setSummary(summary);

      // Move to next undecided
      const nextIndex = await api.getNextUndecidedCluster();
      if (nextIndex !== null) {
        setSelectedIndex(nextIndex);
      }

      setShowCustomLabel(false);
      setCustomLabel('');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleUndo = async () => {
    const undoneIndex = await api.undoLastDecision();
    if (undoneIndex !== null) {
      setSelectedIndex(undoneIndex);

      // Refresh
      const clusters = await api.getClusters();
      review.setClusters(clusters);

      const summary = await api.getReviewSummary();
      review.setSummary(summary);
    }
  };

  const handleSkipAllExisting = async () => {
    if (isSubmitting()) return;
    setIsSubmitting(true);

    try {
      const skippedCount = await api.skipAllExisting();

      // Refresh data
      const clusters = await api.getClusters();
      review.setClusters(clusters);

      const summary = await api.getReviewSummary();
      review.setSummary(summary);

      // Navigate to next undecided
      const nextIndex = await api.getNextUndecidedCluster();
      if (nextIndex !== null) {
        setSelectedIndex(nextIndex);
      }
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleFinish = () => {
    navigation.goTo('filters');
  };

  return (
    <div class="h-full flex gap-6">
      {/* Left Panel: Cluster List */}
      <div class="w-64 flex-shrink-0 card overflow-hidden flex flex-col">
        <div class="p-4 border-b border-gray-200 dark:border-gray-700">
          <h3 class="font-semibold text-gray-900 dark:text-white">Clusters</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400">
            {review.undecidedCount()} remaining
          </p>
        </div>

        <div class="flex-1 overflow-auto">
          <For each={sortedClusters()}>
            {(cluster) => (
              <button
                onClick={() => setSelectedIndex(cluster.index)}
                class="w-full text-left p-3 border-b border-gray-100 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
                classList={{
                  'bg-primary-50 dark:bg-primary-900/30': selectedIndex() === cluster.index,
                }}
              >
                <div class="flex items-center gap-2">
                  <span
                    class="w-5 h-5 rounded flex items-center justify-center text-xs"
                    classList={{
                      'bg-green-100 text-green-700': cluster.decided && cluster.decision === 'Accept',
                      'bg-red-100 text-red-700': cluster.decided && cluster.decision === 'Reject',
                      'bg-gray-100 text-gray-700': cluster.decided && cluster.decision === 'Skip',
                      'bg-gray-200 text-gray-500': !cluster.decided,
                    }}
                  >
                    {cluster.decided ? (cluster.decision === 'Accept' ? '✓' : cluster.decision === 'Reject' ? '✕' : '–') : ''}
                  </span>
                  <span class="font-mono text-sm text-gray-900 dark:text-white truncate flex-1">
                    {cluster.sender_pattern}
                  </span>
                  <Show when={cluster.has_existing_filter}>
                    <span class="text-xs text-yellow-600 dark:text-yellow-400" title="Has existing filter">
                      ★
                    </span>
                  </Show>
                </div>
                <div class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  {cluster.email_count} emails
                </div>
              </button>
            )}
          </For>
        </div>
      </div>

      {/* Center Panel: Cluster Detail */}
      <div class="flex-1 card overflow-hidden flex flex-col">
        <Show
          when={currentCluster()}
          fallback={
            <div class="flex-1 flex items-center justify-center text-gray-500 dark:text-gray-400">
              Select a cluster to review
            </div>
          }
        >
          <div class="flex-1 overflow-auto p-6">
            {/* Header */}
            <div class="mb-6">
              <h2 class="text-xl font-bold text-gray-900 dark:text-white font-mono">
                {currentCluster()!.sender_pattern}
              </h2>
              <p class="text-gray-500 dark:text-gray-400">
                {currentCluster()!.email_count} emails
              </p>
            </div>

            {/* Proposed Filter */}
            <div class="mb-6 p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                Proposed Filter
              </h4>
              <div class="space-y-2 text-sm">
                <div class="flex justify-between">
                  <span class="text-gray-500 dark:text-gray-400">Query:</span>
                  <code class="font-mono text-gray-900 dark:text-white">
                    from:({currentCluster()!.sender_pattern})
                  </code>
                </div>
                <div class="flex justify-between">
                  <span class="text-gray-500 dark:text-gray-400">Label:</span>
                  <span class="text-gray-900 dark:text-white">
                    {currentCluster()!.suggested_label}
                  </span>
                </div>
                <div class="flex justify-between">
                  <span class="text-gray-500 dark:text-gray-400">Archive:</span>
                  <span class="text-gray-900 dark:text-white">
                    {currentCluster()!.should_archive ? 'Yes' : 'No'}
                  </span>
                </div>
              </div>
            </div>

            {/* Existing Filter Comparison */}
            <Show when={currentCluster()!.has_existing_filter}>
              {(() => {
                const cluster = currentCluster()!;
                const currentLabel = cluster.existing_filter_label || '(none)';
                const proposedLabel = cluster.suggested_label;
                const currentArchive = cluster.existing_filter_archive ?? false;
                const proposedArchive = cluster.should_archive;
                const labelChanged = currentLabel !== proposedLabel;
                const archiveChanged = currentArchive !== proposedArchive;

                return (
                  <div class="mb-6 p-4 bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg">
                    <h4 class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-3 flex items-center gap-2">
                      <span>⚠</span>
                      <span>EXISTING FILTER</span>
                      <span class="text-xs font-normal">— [S] keeps current, [Y] updates to proposed</span>
                    </h4>

                    {/* Side-by-side comparison */}
                    <div class="grid grid-cols-2 gap-4 text-sm">
                      {/* Current */}
                      <div>
                        <div class="text-xs font-medium text-yellow-700 dark:text-yellow-300 mb-2">Current</div>
                        <div class="space-y-1">
                          <div class="flex justify-between">
                            <span class="text-yellow-600 dark:text-yellow-400">Label:</span>
                            <span class={labelChanged ? 'text-red-600 dark:text-red-400 font-medium' : 'text-yellow-800 dark:text-yellow-200'}>
                              {currentLabel}
                            </span>
                          </div>
                          <div class="flex justify-between">
                            <span class="text-yellow-600 dark:text-yellow-400">Archive:</span>
                            <span class={archiveChanged ? 'text-blue-600 dark:text-blue-400 font-medium' : 'text-yellow-800 dark:text-yellow-200'}>
                              {currentArchive ? 'YES' : 'NO'}
                            </span>
                          </div>
                        </div>
                      </div>

                      {/* Proposed */}
                      <div>
                        <div class="text-xs font-medium text-yellow-700 dark:text-yellow-300 mb-2">Proposed</div>
                        <div class="space-y-1">
                          <div class="flex justify-between">
                            <span class="text-yellow-600 dark:text-yellow-400">Label:</span>
                            <span class={labelChanged ? 'text-red-600 dark:text-red-400 font-medium' : 'text-yellow-800 dark:text-yellow-200'}>
                              {proposedLabel}
                            </span>
                          </div>
                          <div class="flex justify-between">
                            <span class="text-yellow-600 dark:text-yellow-400">Archive:</span>
                            <span class={archiveChanged ? 'text-blue-600 dark:text-blue-400 font-medium' : 'text-yellow-800 dark:text-yellow-200'}>
                              {proposedArchive ? 'YES' : 'NO'}
                            </span>
                          </div>
                        </div>
                      </div>
                    </div>

                    <Show when={!labelChanged && !archiveChanged}>
                      <div class="mt-3 text-xs text-yellow-600 dark:text-yellow-400">
                        No changes — settings match existing filter
                      </div>
                    </Show>
                  </div>
                );
              })()}
            </Show>

            {/* Sample Emails */}
            <div>
              <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
                Sample Emails
              </h4>
              <div class="space-y-2">
                <For each={currentCluster()!.sample_subjects}>
                  {(subject) => (
                    <div class="p-3 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-600 rounded-lg">
                      <p class="text-sm text-gray-900 dark:text-white truncate">
                        {subject}
                      </p>
                    </div>
                  )}
                </For>
              </div>
            </div>
          </div>

          {/* Custom Label Input */}
          <Show when={showCustomLabel()}>
            <div class="p-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-700">
              <Show when={categories().length > 0 && customLabel().includes('/')}>
                <div class="flex gap-1 mb-2 flex-wrap">
                  <span class="text-xs text-gray-500 dark:text-gray-400 self-center mr-1">Category:</span>
                  <For each={categories()}>
                    {(cat) => {
                      const parts = customLabel().split('/');
                      const isActive = parts.length >= 2 && parts[1] === cat;
                      return (
                        <button
                          class="text-xs px-2 py-1 rounded transition-colors"
                          classList={{
                            'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300': isActive,
                            'bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-500': !isActive,
                          }}
                          onClick={() => {
                            const parts = customLabel().split('/');
                            if (parts.length >= 2) {
                              parts[1] = cat;
                              setCustomLabel(parts.join('/'));
                            }
                          }}
                        >
                          {cat}
                        </button>
                      );
                    }}
                  </For>
                </div>
              </Show>

              <div class="flex gap-2">
                <input
                  type="text"
                  value={customLabel()}
                  onInput={(e) => setCustomLabel(e.currentTarget.value)}
                  placeholder="Enter custom label..."
                  class="input flex-1"
                  autofocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleDecision('customlabel', customLabel());
                    } else if (e.key === 'Escape') {
                      setShowCustomLabel(false);
                      setCustomLabel('');
                    }
                  }}
                />
                <button
                  onClick={() => handleDecision('customlabel', customLabel())}
                  class="btn-primary"
                  disabled={!customLabel()}
                >
                  Apply
                </button>
                <button
                  onClick={() => {
                    setShowCustomLabel(false);
                    setCustomLabel('');
                  }}
                  class="btn-secondary"
                >
                  Cancel
                </button>
              </div>
            </div>
          </Show>

          {/* Action Buttons */}
          <div class="p-4 border-t border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
            <div class="flex flex-wrap gap-2">
              <button
                onClick={() => handleDecision('accept')}
                disabled={isSubmitting()}
                class="btn-success flex items-center gap-2"
              >
                <kbd class="kbd">Y</kbd>
                Accept
              </button>
              <button
                onClick={() => handleDecision('reject')}
                disabled={isSubmitting()}
                class="btn-danger flex items-center gap-2"
              >
                <kbd class="kbd">N</kbd>
                Reject
              </button>
              <button
                onClick={() => handleDecision('skip')}
                disabled={isSubmitting()}
                class="btn-secondary flex items-center gap-2"
              >
                <kbd class="kbd">S</kbd>
                Skip
              </button>
              <button
                onClick={() => handleDecision('delete')}
                disabled={isSubmitting()}
                class="btn-ghost flex items-center gap-2"
              >
                <kbd class="kbd">D</kbd>
                Delete
              </button>
              <button
                onClick={() => handleDecision('exclude')}
                disabled={isSubmitting()}
                class="btn-ghost flex items-center gap-2"
              >
                <kbd class="kbd">E</kbd>
                Exclude
              </button>
              <button
                onClick={() => {
                  setCustomLabel(currentCluster()?.suggested_label || '');
                  setShowCustomLabel(true);
                }}
                disabled={isSubmitting()}
                class="btn-ghost flex items-center gap-2"
              >
                <kbd class="kbd">L</kbd>
                Custom Label
              </button>
              <button
                onClick={() => {
                  const idx = selectedIndex();
                  if (idx === null) return;
                  const updated = review.clusters().map((c, i) =>
                    i === idx ? { ...c, should_archive: !c.should_archive } : c
                  );
                  review.setClusters(updated);
                }}
                disabled={isSubmitting()}
                class="btn-ghost flex items-center gap-2"
              >
                <kbd class="kbd">A</kbd>
                Archive: {currentCluster()?.should_archive ? 'ON' : 'OFF'}
              </button>
            </div>
          </div>
        </Show>
      </div>

      {/* Right Panel: Summary */}
      <div class="w-64 flex-shrink-0 card p-4 space-y-6">
        <div>
          <h3 class="font-semibold text-gray-900 dark:text-white mb-4">
            Review Summary
          </h3>

          <Show when={review.summary()}>
            <div class="space-y-3 text-sm">
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Accepted:</span>
                <span class="text-green-600 dark:text-green-400 font-medium">
                  {review.summary()!.accepted}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Rejected:</span>
                <span class="text-red-600 dark:text-red-400 font-medium">
                  {review.summary()!.rejected}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Skipped:</span>
                <span class="text-gray-600 dark:text-gray-400 font-medium">
                  {review.summary()!.skipped}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Remaining:</span>
                <span class="text-primary-600 dark:text-primary-400 font-medium">
                  {review.summary()!.remaining}
                </span>
              </div>
            </div>
          </Show>
        </div>

        {/* Existing Filters Section */}
        <Show when={review.summary() && review.summary()!.existing_filters > 0}>
          <div class="p-3 bg-yellow-50 dark:bg-yellow-900/30 rounded-lg">
            <div class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-2">
              Existing Filters
            </div>
            <div class="text-xs text-yellow-700 dark:text-yellow-300 space-y-1">
              <div class="flex justify-between">
                <span>Total:</span>
                <span class="font-medium">{review.summary()!.existing_filters}</span>
              </div>
              <div class="flex justify-between">
                <span>Remaining:</span>
                <span class="font-medium">{review.summary()!.existing_remaining}</span>
              </div>
            </div>
            <Show when={review.summary()!.existing_remaining > 0}>
              <button
                onClick={handleSkipAllExisting}
                disabled={isSubmitting()}
                class="mt-2 w-full btn-secondary text-xs py-1.5 flex items-center justify-center gap-1"
              >
                <kbd class="kbd text-[10px]">⇧S</kbd>
                Skip All Existing ({review.summary()!.existing_remaining})
              </button>
            </Show>
          </div>
        </Show>

        <hr class="border-gray-200 dark:border-gray-700" />

        <div class="space-y-2">
          <button
            onClick={handleUndo}
            class="btn-ghost w-full flex items-center justify-center gap-2"
          >
            <kbd class="kbd">U</kbd>
            Undo Last
          </button>

          <button
            onClick={handleFinish}
            class="btn-primary w-full"
          >
            Finish Review
          </button>
        </div>

        <hr class="border-gray-200 dark:border-gray-700" />

        <div>
          <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Keyboard Shortcuts
          </h4>
          <div class="text-xs text-gray-500 dark:text-gray-400 space-y-1">
            <div><kbd class="kbd">Y</kbd> / <kbd class="kbd">Enter</kbd> Accept</div>
            <div><kbd class="kbd">N</kbd> Reject</div>
            <div><kbd class="kbd">S</kbd> Skip</div>
            <div><kbd class="kbd">⇧S</kbd> Skip all existing</div>
            <div><kbd class="kbd">D</kbd> Delete filter</div>
            <div><kbd class="kbd">E</kbd> Exclude forever</div>
            <div><kbd class="kbd">L</kbd> Custom label</div>
            <div><kbd class="kbd">A</kbd> Toggle archive</div>
            <div><kbd class="kbd">U</kbd> Undo</div>
            <div><kbd class="kbd">↑↓</kbd> Navigate</div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default ReviewView;
