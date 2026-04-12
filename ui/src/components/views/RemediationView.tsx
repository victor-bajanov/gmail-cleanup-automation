import { Component, Show, For, createSignal, createMemo } from 'solid-js';
import { remediation } from '../../stores/app';
import * as api from '../../lib/api';
import type { OverlapGroup, GroupDecision } from '../../types';

const RemediationView: Component = () => {
  const [hasDetected, setHasDetected] = createSignal(false);

  const handleDetect = async () => {
    remediation.setPhase('detecting');
    remediation.setError(null);
    try {
      const groups = await api.detectOverlaps();
      remediation.setGroups(groups);
      setHasDetected(true);
      if (groups.length > 0) {
        remediation.setPhase('deciding');
      } else {
        remediation.setPhase('idle');
      }
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
      remediation.setPhase('idle');
    }
  };

  const handleAccept = async (group: OverlapGroup) => {
    remediation.setError(null);
    const resType = group.resolution_type;
    let decision: GroupDecision;
    if (typeof resType === 'object' && 'Consolidate' in resType) {
      decision = { Consolidate: resType.Consolidate };
    } else if (typeof resType === 'object' && 'MechanicalFix' in resType) {
      decision = { ReplaceWithExclusive: { replacement_filters: resType.MechanicalFix.proposed_replacements } };
    } else {
      return;
    }
    try {
      await api.submitGroupDecision(group.group_id, decision);
      remediation.setDecision(group.group_id, decision);
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
    }
  };

  const handlePickWinner = async (group: OverlapGroup, filterId: string) => {
    remediation.setError(null);
    const decision: GroupDecision = { KeepOne: { keep_filter_id: filterId } };
    try {
      await api.submitGroupDecision(group.group_id, decision);
      remediation.setDecision(group.group_id, decision);
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
    }
  };

  const getPickedFilterId = (groupId: string): string | null => {
    const d = remediation.decisions()[groupId];
    if (d && typeof d === 'object' && 'KeepOne' in d) {
      return d.KeepOne.keep_filter_id;
    }
    return null;
  };

  const handleReviewPlan = async () => {
    remediation.setError(null);
    try {
      const summary = await api.remediationSummary();
      remediation.setSummary(summary);
      remediation.setPhase('confirming');
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
    }
  };

  const handleExecute = async () => {
    remediation.setError(null);
    remediation.setPhase('executing');
    try {
      const result = await api.executeRemediation();
      remediation.setResult(result);
      const swaps = await api.collectRemediationSwaps();
      remediation.setSwaps(swaps);
      remediation.setPhase('results');
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
      remediation.setPhase('confirming');
    }
  };

  const handleApplySwaps = async () => {
    remediation.setError(null);
    try {
      const result = await api.applyRemediationSwaps();
      remediation.setApplyResult(result);
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
    }
  };

  const handleSkip = async (group: OverlapGroup) => {
    remediation.setError(null);
    try {
      await api.submitGroupDecision(group.group_id, 'Skip');
      remediation.setDecision(group.group_id, 'Skip');
    } catch (e: any) {
      remediation.setError(e?.message ?? String(e));
    }
  };

  const getDecisionState = (groupId: string): 'undecided' | 'decided' | 'skipped' => {
    const d = remediation.decisions()[groupId];
    if (!d) return 'undecided';
    if (d === 'Skip') return 'skipped';
    return 'decided';
  };

  const borderClass = (groupId: string) => {
    const state = getDecisionState(groupId);
    if (state === 'decided') return 'border-l-green-500';
    if (state === 'skipped') return 'border-l-yellow-400';
    return 'border-l-gray-300 dark:border-l-gray-600';
  };

  const isConsolidate = (group: OverlapGroup) => {
    const r = group.resolution_type;
    return typeof r === 'object' && 'Consolidate' in r;
  };

  const isMechanicalFix = (group: OverlapGroup) => {
    const r = group.resolution_type;
    return typeof r === 'object' && 'MechanicalFix' in r;
  };

  const isPickWinner = (group: OverlapGroup) => {
    return group.resolution_type === 'PickWinner';
  };

  const redundantCount = (group: OverlapGroup) => {
    const r = group.resolution_type;
    if (typeof r === 'object' && 'Consolidate' in r) {
      return r.Consolidate.remove_filter_ids.length;
    }
    return 0;
  };

  return (
    <div data-testid="remediation-view" class="max-w-4xl mx-auto space-y-6">
      <h2 class="text-xl font-bold text-gray-900 dark:text-white">
        Filter Remediation
      </h2>

      {/* Error display */}
      <Show when={remediation.error()}>
        <div class="p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg text-red-700 dark:text-red-300">
          {remediation.error()}
        </div>
      </Show>

      {/* Idle / Detect phase */}
      <Show when={remediation.phase() === 'idle' || remediation.phase() === 'detecting'}>
        <div class="space-y-4">
          <p class="text-gray-600 dark:text-gray-400">
            Scan your Gmail filters for overlapping rules that can be consolidated or simplified.
          </p>

          <Show when={hasDetected() && remediation.groups().length === 0}>
            <div data-testid="no-overlaps" class="p-4 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-lg text-green-700 dark:text-green-300">
              No overlapping filters found. Your filters look clean!
            </div>
          </Show>

          <button
            data-testid="detect-btn"
            class="btn-primary"
            onClick={handleDetect}
            disabled={remediation.phase() === 'detecting'}
          >
            <Show when={remediation.phase() === 'detecting'} fallback="Detect Overlaps">
              <span class="inline-block animate-spin mr-2">&#8635;</span>
              Detecting...
            </Show>
          </button>
        </div>
      </Show>

      {/* Decide phase */}
      <Show when={remediation.phase() === 'deciding'}>
        <div class="space-y-4">
          <h3 data-testid="decide-header" class="text-lg font-semibold text-gray-800 dark:text-gray-200">
            {remediation.groups().length} overlap group{remediation.groups().length !== 1 ? 's' : ''} found
          </h3>

          <For each={remediation.groups()}>
            {(group) => (
              <div
                data-testid={`group-card-${group.group_id}`}
                class={`card border-l-4 ${borderClass(group.group_id)} p-4 relative`}
              >
                {/* Decision badge */}
                <Show when={getDecisionState(group.group_id) !== 'undecided'}>
                  <span class={`absolute top-2 right-2 text-xs font-medium px-2 py-1 rounded ${
                    getDecisionState(group.group_id) === 'decided'
                      ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                      : 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400'
                  }`}>
                    {getDecisionState(group.group_id) === 'decided' ? 'Accepted' : 'Skipped'}
                  </span>
                </Show>

                <div class="space-y-2">
                  <div class="font-medium text-gray-900 dark:text-white">
                    {group.from_pattern}
                  </div>

                  <div class="text-sm text-gray-500 dark:text-gray-400">
                    Labels: {group.label_names.join(', ')} &middot; {group.filters.length} filter{group.filters.length !== 1 ? 's' : ''}
                  </div>

                  <Show when={isConsolidate(group)}>
                    <div class="text-sm text-gray-600 dark:text-gray-300">
                      Same label &mdash; {redundantCount(group)} redundant filter{redundantCount(group) !== 1 ? 's' : ''}
                    </div>
                  </Show>

                  <Show when={isMechanicalFix(group)}>
                    <div class="text-sm text-gray-600 dark:text-gray-300">
                      Auto-fixable &mdash; add subject exclusions
                    </div>
                    <div class="mt-2 space-y-1">
                      <For each={typeof group.resolution_type === 'object' && 'MechanicalFix' in group.resolution_type ? group.resolution_type.MechanicalFix.proposed_replacements : []}>
                        {(rule) => (
                          <div class="text-xs font-mono bg-gray-50 dark:bg-gray-800 rounded px-2 py-1 text-gray-700 dark:text-gray-300">
                            {rule.from_pattern} &rarr; {rule.name}
                            <Show when={rule.excluded_subject_patterns.length > 0}>
                              {' '}<span class="text-red-500">-[{rule.excluded_subject_patterns.join(', ')}]</span>
                            </Show>
                            <Show when={rule.subject_keywords.length > 0}>
                              {' '}<span class="text-green-500">+[{rule.subject_keywords.join(', ')}]</span>
                            </Show>
                          </div>
                        )}
                      </For>
                    </div>
                  </Show>

                  <Show when={isPickWinner(group)}>
                    <div class="text-sm text-gray-600 dark:text-gray-300">
                      Different labels &mdash; pick which filter to keep
                    </div>
                    <div class="mt-2 space-y-1">
                      <For each={group.filters}>
                        {(filter) => (
                          <button
                            data-testid={`filter-row-${filter.id}`}
                            class={`w-full text-left text-sm rounded border px-3 py-2 transition ${
                              getPickedFilterId(group.group_id) === filter.id
                                ? 'ring-2 ring-primary-300 border-primary-500 bg-primary-50 dark:bg-primary-900/30'
                                : 'border-gray-200 dark:border-gray-600 hover:border-gray-300'
                            }`}
                            onClick={() => handlePickWinner(group, filter.id)}
                          >
                            <span class="font-medium text-gray-900 dark:text-white">{filter.from || filter.query || 'All'}</span>
                            <Show when={filter.subject}>
                              <span class="ml-2 text-gray-500">subject: {filter.subject}</span>
                            </Show>
                            <span class="ml-2 text-gray-400">&rarr; {filter.add_label_ids.join(', ')}</span>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>

                  {/* Action buttons - hidden after decision */}
                  <Show when={getDecisionState(group.group_id) === 'undecided'}>
                    <div class="flex gap-2 mt-3">
                      <Show when={isConsolidate(group)}>
                        <button
                          data-testid={`accept-${group.group_id}`}
                          class="btn-primary text-sm"
                          onClick={() => handleAccept(group)}
                        >
                          Accept
                        </button>
                      </Show>
                      <Show when={isMechanicalFix(group)}>
                        <button
                          data-testid={`accept-${group.group_id}`}
                          class="btn-primary text-sm"
                          onClick={() => handleAccept(group)}
                        >
                          Accept Fix
                        </button>
                      </Show>
                      <button
                        data-testid={`skip-${group.group_id}`}
                        class="btn-secondary text-sm"
                        onClick={() => handleSkip(group)}
                      >
                        Skip
                      </button>
                    </div>
                  </Show>
                </div>
              </div>
            )}
          </For>

          {/* Sticky footer */}
          <div class="sticky bottom-0 bg-white dark:bg-gray-800 border-t border-gray-200 dark:border-gray-700 p-4 mt-4 -mx-4 flex items-center justify-between rounded-b-lg">
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
        </div>
      </Show>

      {/* Confirm phase */}
      <Show when={remediation.phase() === 'confirming'}>
        <div class="card p-6 space-y-4">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
            Remediation Plan
          </h3>
          <pre
            data-testid="plan-summary"
            class="text-sm font-mono bg-gray-50 dark:bg-gray-700 p-4 rounded-lg whitespace-pre-wrap text-gray-700 dark:text-gray-300"
          >{remediation.summary()}</pre>
          <div class="flex gap-3">
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

      {/* Executing phase */}
      <Show when={remediation.phase() === 'executing'}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Executing remediation plan...</p>
        </div>
      </Show>

      {/* Results phase */}
      <Show when={remediation.phase() === 'results'}>
        <div class="space-y-4">
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

          {/* Label swaps section */}
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
    </div>
  );
};

export default RemediationView;
