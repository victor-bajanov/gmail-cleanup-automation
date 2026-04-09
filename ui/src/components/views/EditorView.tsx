import { Component, createMemo, createSignal, For, Show, onMount } from 'solid-js';
import { editor } from '../../stores/app';
import * as api from '../../lib/api';
import type { EditorFilter, FilterAction, ActionDiff } from '../../types';

const EditorView: Component = () => {
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [resultMessage, setResultMessage] = createSignal<string | null>(null);
  const [dryRunResults, setDryRunResults] = createSignal<ActionDiff[] | null>(null);

  const loadFilters = async (refresh = false) => {
    editor.setLoading(true);
    setError(null);
    try {
      const resp = await api.editorGetFilters(refresh);
      editor.setFilters(resp.filters);
      editor.setLabelMap(resp.label_map);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      editor.setLoading(false);
    }
  };

  onMount(async () => {
    if (editor.filters().length === 0) {
      await loadFilters(); // uses cached state from FiltersView if available
    }
  });

  // Local search filtering (includes resolved label names)
  const filteredFilters = createMemo(() => {
    const term = editor.search().toLowerCase().trim();
    if (!term) return editor.filters();
    return editor.filters().filter((f) => {
      const resolvedLabels = f.add_label_ids.map((id) => editor.resolveLabel(id));
      const fields = [
        f.query,
        f.from,
        f.to,
        f.subject,
        ...f.add_label_ids,
        ...resolvedLabels,
      ]
        .filter(Boolean)
        .map((s) => s!.toLowerCase());
      return fields.some((s) => s.includes(term));
    });
  });

  const queuedActionForFilter = (filterId: string): FilterAction | undefined => {
    return editor.queue().find((a) => a.filter_id === filterId);
  };

  const hasArchive = (f: EditorFilter): boolean => {
    return f.remove_label_ids.some((id) => id === 'INBOX');
  };

  const toggleArchive = (f: EditorFilter) => {
    const currentlyArchives = hasArchive(f);
    editor.addAction({
      type: 'update_archive',
      filter_id: f.id,
      new_value: !currentlyArchives,
    });
  };

  const queueDelete = (f: EditorFilter) => {
    editor.addAction({
      type: 'delete',
      filter_id: f.id,
    });
  };

  const undoAction = (filterId: string) => {
    editor.removeAction(filterId);
  };

  const handleDryRun = async () => {
    setError(null);
    try {
      const results = await api.editorDryRun(editor.queue());
      setDryRunResults(results);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleApply = async () => {
    setError(null);
    setResultMessage(null);
    try {
      const result = await api.editorApply(editor.queue());
      editor.clearQueue();
      await loadFilters(true);
      if (result.failed.length > 0) {
        setResultMessage(
          `Applied ${result.succeeded} changes, ${result.failed.length} failed: ${result.failed.map(([id, err]) => `${id}: ${err}`).join('; ')}`
        );
      } else {
        setResultMessage(`Successfully applied ${result.succeeded} changes.`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const getFilterDisplayName = (f: EditorFilter): string => {
    return f.from || f.query || f.to || f.subject || f.id;
  };

  return (
    <div class="flex gap-6 h-full">
      {/* Left/Main Panel */}
      <div class="flex-1 min-w-0 space-y-4">
        {/* Header */}
        <div class="flex items-center justify-between">
          <div>
            <h2 class="text-xl font-bold text-gray-900 dark:text-white">
              Filter Editor
            </h2>
            <p class="text-gray-500 dark:text-gray-400">
              Edit, delete, and manage Gmail filters
            </p>
          </div>
          <button
            onClick={() => loadFilters(true)}
            disabled={editor.isLoading()}
            class="btn-secondary"
          >
            {editor.isLoading() ? 'Loading...' : 'Refresh'}
          </button>
        </div>

        {/* Error */}
        <Show when={error()}>
          <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
            <p class="text-red-700 dark:text-red-300">{error()}</p>
          </div>
        </Show>

        {/* Result message */}
        <Show when={resultMessage()}>
          <div class="card p-4 border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30">
            <p class="text-green-700 dark:text-green-300">{resultMessage()}</p>
          </div>
        </Show>

        {/* Search + count */}
        <div class="flex items-center gap-4">
          <div class="flex-1">
            <input
              type="text"
              placeholder="Search filters (query, from, to, subject, labels)..."
              value={editor.search()}
              onInput={(e) => editor.setSearch(e.currentTarget.value)}
              class="w-full px-4 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-900 dark:text-white placeholder-gray-400 dark:placeholder-gray-500 focus:ring-2 focus:ring-primary-500 focus:border-transparent outline-none"
            />
          </div>
          <span class="text-sm text-gray-500 dark:text-gray-400 whitespace-nowrap">
            {filteredFilters().length} of {editor.filters().length} filters
          </span>
        </div>

        {/* Loading */}
        <Show when={editor.isLoading()}>
          <div class="card p-8 text-center">
            <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
            <p class="text-gray-500 dark:text-gray-400">Loading filters...</p>
          </div>
        </Show>

        {/* Filter list */}
        <Show when={!editor.isLoading()}>
          <div class="space-y-2 overflow-auto max-h-[calc(100vh-280px)]">
            <For each={filteredFilters()}>
              {(f) => {
                const action = () => queuedActionForFilter(f.id);
                const isExpanded = () => expandedId() === f.id;

                return (
                  <div
                    class="card p-3 border-2 cursor-pointer transition-colors"
                    classList={{
                      'border-red-400 dark:border-red-600': action()?.type === 'delete',
                      'border-orange-400 dark:border-orange-600': !!action() && action()?.type !== 'delete',
                      'border-gray-200 dark:border-gray-700': !action(),
                    }}
                    onClick={() => setExpandedId(isExpanded() ? null : f.id)}
                  >
                    {/* Row summary */}
                    <div class="flex items-center gap-3">
                      <span class="font-mono text-sm text-gray-900 dark:text-white truncate flex-1 min-w-0">
                        {getFilterDisplayName(f)}
                      </span>

                      {/* Label badges (resolved to names) */}
                      <div class="flex items-center gap-1 flex-shrink-0">
                        <For each={f.add_label_ids}>
                          {(labelId) => (
                            <span class="text-xs px-2 py-0.5 rounded-full bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300">
                              {editor.resolveLabel(labelId)}
                            </span>
                          )}
                        </For>
                      </div>

                      {/* Archive badge */}
                      <span
                        class="text-xs px-2 py-0.5 rounded-full flex-shrink-0"
                        classList={{
                          'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300': hasArchive(f),
                          'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400': !hasArchive(f),
                        }}
                      >
                        {hasArchive(f) ? 'Archive' : 'Inbox'}
                      </span>

                      {/* Queued badge */}
                      <Show when={action()}>
                        <span class="text-xs px-2 py-0.5 rounded-full bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-300 flex-shrink-0">
                          Queued
                        </span>
                      </Show>
                    </div>

                    {/* Expanded detail panel */}
                    <Show when={isExpanded()}>
                      <div
                        class="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700 space-y-3"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {/* Filter fields */}
                        <div class="grid grid-cols-2 gap-2 text-sm">
                          <Show when={f.from}>
                            <div>
                              <span class="text-gray-500 dark:text-gray-400">From: </span>
                              <span class="font-mono text-gray-900 dark:text-white">{f.from}</span>
                            </div>
                          </Show>
                          <Show when={f.to}>
                            <div>
                              <span class="text-gray-500 dark:text-gray-400">To: </span>
                              <span class="font-mono text-gray-900 dark:text-white">{f.to}</span>
                            </div>
                          </Show>
                          <Show when={f.subject}>
                            <div>
                              <span class="text-gray-500 dark:text-gray-400">Subject: </span>
                              <span class="font-mono text-gray-900 dark:text-white">{f.subject}</span>
                            </div>
                          </Show>
                          <Show when={f.query}>
                            <div>
                              <span class="text-gray-500 dark:text-gray-400">Query: </span>
                              <span class="font-mono text-gray-900 dark:text-white">{f.query}</span>
                            </div>
                          </Show>
                        </div>

                        {/* Action buttons */}
                        <div class="flex items-center gap-2">
                          <button
                            onClick={() => toggleArchive(f)}
                            class="text-sm px-3 py-1.5 rounded-lg transition-colors"
                            classList={{
                              'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 hover:bg-green-200 dark:hover:bg-green-900/50': !hasArchive(f),
                              'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600': hasArchive(f),
                            }}
                          >
                            {hasArchive(f) ? 'Disable Archive' : 'Enable Archive'}
                          </button>

                          <button
                            onClick={() => queueDelete(f)}
                            class="text-sm px-3 py-1.5 rounded-lg bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300 hover:bg-red-200 dark:hover:bg-red-900/50 transition-colors"
                          >
                            Delete
                          </button>

                          <Show when={action()}>
                            <button
                              onClick={() => undoAction(f.id)}
                              class="text-sm px-3 py-1.5 rounded-lg bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-300 hover:bg-yellow-200 dark:hover:bg-yellow-900/50 transition-colors"
                            >
                              Undo
                            </button>
                          </Show>
                        </div>
                      </div>
                    </Show>
                  </div>
                );
              }}
            </For>

            <Show when={filteredFilters().length === 0 && !editor.isLoading()}>
              <div class="card p-8 text-center">
                <p class="text-gray-500 dark:text-gray-400">
                  {editor.search() ? 'No filters match your search.' : 'No filters found. Click Refresh to load.'}
                </p>
              </div>
            </Show>
          </div>
        </Show>
      </div>

      {/* Right Sidebar: Action Queue */}
      <div class="w-80 flex-shrink-0 flex flex-col">
        <div class="card p-4 flex flex-col h-full">
          {/* Queue header */}
          <div class="flex items-center justify-between mb-4">
            <div class="flex items-center gap-2">
              <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
                Action Queue
              </h3>
              <span class="text-xs px-2 py-0.5 rounded-full bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300">
                {editor.queueCount()}
              </span>
            </div>
            <Show when={editor.queueCount() > 0}>
              <button
                onClick={() => editor.clearQueue()}
                class="text-xs text-red-600 dark:text-red-400 hover:underline"
              >
                Clear All
              </button>
            </Show>
          </div>

          {/* Queue list */}
          <div class="flex-1 overflow-auto space-y-2 min-h-0">
            <For each={editor.queue()}>
              {(action) => (
                <div class="p-2 bg-gray-50 dark:bg-gray-700 rounded-lg flex items-center gap-2">
                  <span
                    class="text-xs px-2 py-0.5 rounded flex-shrink-0"
                    classList={{
                      'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300': action.type === 'delete',
                      'bg-orange-100 dark:bg-orange-900/30 text-orange-700 dark:text-orange-300': action.type === 'update_archive',
                      'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300': action.type === 'update_labels',
                    }}
                  >
                    {action.type === 'delete' ? 'DEL' : action.type === 'update_archive' ? 'ARC' : 'LBL'}
                  </span>
                  <span class="text-xs font-mono text-gray-700 dark:text-gray-300 truncate flex-1 min-w-0">
                    {action.filter_id.substring(0, 16)}...
                  </span>
                  <button
                    onClick={() => editor.removeAction(action.filter_id)}
                    class="text-gray-400 hover:text-red-500 dark:hover:text-red-400 flex-shrink-0"
                  >
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>
              )}
            </For>

            <Show when={editor.queueCount() === 0}>
              <p class="text-sm text-gray-400 dark:text-gray-500 text-center py-8">
                No actions queued. Select a filter and make changes.
              </p>
            </Show>
          </div>

          {/* Bottom action bar */}
          <div class="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700 space-y-2">
            <button
              onClick={handleDryRun}
              disabled={editor.queueCount() === 0}
              class="btn-secondary w-full"
            >
              Dry Run
            </button>
            <button
              onClick={handleApply}
              disabled={editor.queueCount() === 0}
              class="btn-primary w-full"
            >
              Apply {editor.queueCount()} Change{editor.queueCount() !== 1 ? 's' : ''}
            </button>
          </div>
        </div>
      </div>

      {/* Dry Run Modal */}
      <Show when={dryRunResults()}>
        <div class="fixed inset-0 z-50 flex items-center justify-center">
          {/* Backdrop */}
          <div
            class="absolute inset-0 bg-black/50"
            onClick={() => setDryRunResults(null)}
          />

          {/* Modal */}
          <div class="relative bg-white dark:bg-gray-800 rounded-xl shadow-xl max-w-2xl w-full mx-4 max-h-[80vh] overflow-hidden">
            {/* Header */}
            <div class="flex items-center justify-between p-4 border-b border-gray-200 dark:border-gray-700">
              <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
                Dry Run Results ({dryRunResults()!.length} actions)
              </h3>
              <button
                onClick={() => setDryRunResults(null)}
                class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
              >
                <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            {/* Content */}
            <div class="p-4 overflow-y-auto max-h-[60vh] space-y-3">
              <Show when={dryRunResults()!.length === 0}>
                <p class="text-center text-gray-500 dark:text-gray-400 py-8">
                  No changes to preview.
                </p>
              </Show>

              <For each={dryRunResults()!}>
                {(diff) => (
                  <div class="p-4 rounded-lg border border-gray-200 dark:border-gray-700 space-y-2">
                    <div class="flex items-center gap-2">
                      <span
                        class="text-xs px-2 py-0.5 rounded font-medium"
                        classList={{
                          'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300': diff.action_type !== 'delete',
                          'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300': diff.action_type === 'delete',
                        }}
                      >
                        {diff.action_type === 'delete' ? 'Delete' : 'Update'}
                      </span>
                      <span class="text-sm font-mono text-gray-700 dark:text-gray-300 truncate">
                        {diff.filter_id}
                      </span>
                    </div>
                    <p class="text-sm text-gray-600 dark:text-gray-400">
                      {diff.description}
                    </p>

                    <Show when={diff.changes.length > 0}>
                      <div class="space-y-1 mt-2">
                        <For each={diff.changes}>
                          {(change) => (
                            <div class="text-sm font-mono">
                              <span class="text-gray-500 dark:text-gray-400">{change.field}: </span>
                              <span class="line-through text-red-500 dark:text-red-400">{change.before}</span>
                              {' '}
                              <span class="text-green-600 dark:text-green-400">{change.after}</span>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                )}
              </For>
            </div>

            {/* Footer */}
            <div class="p-4 border-t border-gray-200 dark:border-gray-700">
              <button
                onClick={() => setDryRunResults(null)}
                class="btn-secondary w-full"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default EditorView;
