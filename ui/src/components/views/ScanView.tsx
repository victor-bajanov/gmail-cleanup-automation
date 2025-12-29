import { Component, createSignal, Show, For } from 'solid-js';
import { scan, navigation, review } from '../../stores/app';
import * as api from '../../lib/api';
import type { ScanOptions, ScanResult, DomainStat } from '../../types';

const ScanView: Component = () => {
  const [periodDays, setPeriodDays] = createSignal(30);
  const [minClusterSize, setMinClusterSize] = createSignal(3);
  const [scanResult, setScanResult] = createSignal<ScanResult | null>(null);
  const [domainStats, setDomainStats] = createSignal<DomainStat[]>([]);
  const [error, setError] = createSignal<string | null>(null);

  const handleScan = async () => {
    setError(null);
    setScanResult(null);
    scan.setScanning(true);

    const options: ScanOptions = {
      period_days: periodDays(),
      min_cluster_size: minClusterSize(),
    };

    try {
      const result = await api.scanEmails(options);
      setScanResult(result);

      // Get domain stats
      const stats = await api.getDomainStats();
      setDomainStats(stats.slice(0, 20));

      // Get clusters for review
      const clusters = await api.getClusters(minClusterSize());
      review.setClusters(clusters);

      // Get initial summary
      const summary = await api.getReviewSummary();
      review.setSummary(summary);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      scan.setScanning(false);
    }
  };

  const handleStartReview = () => {
    navigation.goTo('review');
  };

  const progressPercentage = () => {
    const progress = scan.progress();
    if (!progress || progress.total === 0) return 0;
    return Math.round((progress.current / progress.total) * 100);
  };

  return (
    <div class="max-w-4xl mx-auto space-y-6">
      {/* Configuration Card */}
      <div class="card p-6">
        <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
          Scan Configuration
        </h3>

        <div class="grid grid-cols-2 gap-6">
          <div>
            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Scan Period (days)
            </label>
            <input
              type="number"
              value={periodDays()}
              onInput={(e) => setPeriodDays(parseInt(e.currentTarget.value) || 30)}
              min="1"
              max="365"
              class="input"
              disabled={scan.isScanning()}
            />
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Min Emails per Cluster
            </label>
            <input
              type="number"
              value={minClusterSize()}
              onInput={(e) => setMinClusterSize(parseInt(e.currentTarget.value) || 3)}
              min="1"
              max="100"
              class="input"
              disabled={scan.isScanning()}
            />
          </div>
        </div>

        <div class="mt-6">
          <button
            onClick={handleScan}
            disabled={scan.isScanning()}
            class="btn-primary"
          >
            {scan.isScanning() ? 'Scanning...' : 'Start Scan'}
          </button>
        </div>
      </div>

      {/* Progress */}
      <Show when={scan.isScanning() || scan.progress()}>
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Scan Progress
          </h3>

          <div class="space-y-4">
            {/* Progress bar */}
            <div class="relative">
              <div class="h-2 bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden">
                <div
                  class="h-full bg-primary-600 transition-all duration-300"
                  style={{ width: `${progressPercentage()}%` }}
                />
              </div>
              <div class="mt-2 flex justify-between text-sm text-gray-600 dark:text-gray-400">
                <span>
                  {scan.progress()?.phase === 'complete'
                    ? 'Complete'
                    : scan.progress()?.phase?.replace('_', ' ') || 'Starting...'}
                </span>
                <span>
                  {scan.progress()?.current || 0} / {scan.progress()?.total || 0}
                </span>
              </div>
            </div>

            <Show when={scan.progress()?.message}>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                {scan.progress()!.message}
              </p>
            </Show>
          </div>
        </div>
      </Show>

      {/* Error */}
      <Show when={error()}>
        <div class="card p-6 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{error()}</p>
        </div>
      </Show>

      {/* Results */}
      <Show when={scanResult()}>
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Scan Results
          </h3>

          <div class="grid grid-cols-4 gap-4 mb-6">
            <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div class="text-2xl font-bold text-gray-900 dark:text-white">
                {scanResult()!.total_messages}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Messages Found</div>
            </div>
            <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div class="text-2xl font-bold text-gray-900 dark:text-white">
                {scanResult()!.fetched_messages}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Fetched</div>
            </div>
            <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div class="text-2xl font-bold text-primary-600 dark:text-primary-400">
                {scanResult()!.cluster_count}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Clusters</div>
            </div>
            <div class="text-center p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div class="text-2xl font-bold text-gray-900 dark:text-white">
                {scanResult()!.unique_domains}
              </div>
              <div class="text-sm text-gray-500 dark:text-gray-400">Unique Domains</div>
            </div>
          </div>

          <Show when={scanResult()!.cluster_count > 0}>
            <button onClick={handleStartReview} class="btn-success">
              Review {scanResult()!.cluster_count} Clusters
            </button>
          </Show>
        </div>
      </Show>

      {/* Top Domains */}
      <Show when={domainStats().length > 0}>
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Top Domains
          </h3>

          <div class="space-y-2">
            <For each={domainStats()}>
              {(stat) => (
                <div class="flex items-center justify-between py-2 border-b border-gray-100 dark:border-gray-700 last:border-0">
                  <span class="font-mono text-sm text-gray-900 dark:text-white">
                    {stat.domain}
                  </span>
                  <span class="text-sm text-gray-500 dark:text-gray-400">
                    {stat.count} emails
                  </span>
                </div>
              )}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ScanView;
