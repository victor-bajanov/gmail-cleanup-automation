import { Component, createSignal, onMount, onCleanup, Show } from 'solid-js';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import ProgressBar from '../ui/ProgressBar';
import PhaseIndicator, { Phase } from '../ui/PhaseIndicator';
import LiveStats, { Stat } from '../ui/LiveStats';
import type { ScanProgress, FilterProgress } from '../../types';

export interface ProgressViewProps {
  mode: 'scan' | 'filter';
  onComplete?: () => void;
  onCancel?: () => void;
}

const SCAN_PHASES: Phase[] = [
  { id: 'listing', label: 'Listing', description: 'Finding messages' },
  { id: 'fetching', label: 'Fetching', description: 'Downloading metadata' },
  { id: 'classifying', label: 'Classifying', description: 'Analyzing senders' },
  { id: 'clustering', label: 'Clustering', description: 'Grouping emails' },
  { id: 'complete', label: 'Complete', description: 'Done' },
];

const FILTER_PHASES: Phase[] = [
  { id: 'fetching_existing', label: 'Fetching', description: 'Getting existing filters' },
  { id: 'analyzing_overlaps', label: 'Analyzing', description: 'Checking overlaps' },
  { id: 'creating', label: 'Creating', description: 'Creating filters' },
  { id: 'applying_retroactive', label: 'Applying', description: 'Labeling emails' },
  { id: 'complete', label: 'Complete', description: 'Done' },
];

const ProgressView: Component<ProgressViewProps> = (props) => {
  const [scanProgress, setScanProgress] = createSignal<ScanProgress | null>(null);
  const [filterProgress, setFilterProgress] = createSignal<FilterProgress | null>(null);
  const [completedPhases, setCompletedPhases] = createSignal<string[]>([]);
  const [startTime, setStartTime] = createSignal<number | null>(null);
  const [elapsedTime, setElapsedTime] = createSignal(0);

  let timerInterval: number | undefined;
  let unlistenScan: UnlistenFn | undefined;
  let unlistenFilter: UnlistenFn | undefined;

  onMount(async () => {
    setStartTime(Date.now());

    // Start timer
    timerInterval = window.setInterval(() => {
      const start = startTime();
      if (start) {
        setElapsedTime(Math.floor((Date.now() - start) / 1000));
      }
    }, 1000);

    // Listen for scan progress
    unlistenScan = await listen<ScanProgress>('scan-progress', (event) => {
      const progress = event.payload;
      setScanProgress(progress);

      // Track completed phases
      if (progress.phase !== 'complete') {
        const phaseIndex = SCAN_PHASES.findIndex(p => p.id === progress.phase);
        if (phaseIndex > 0) {
          const completed = SCAN_PHASES.slice(0, phaseIndex).map(p => p.id);
          setCompletedPhases(completed);
        }
      } else {
        setCompletedPhases(SCAN_PHASES.map(p => p.id));
        props.onComplete?.();
      }
    });

    // Listen for filter progress
    unlistenFilter = await listen<FilterProgress>('filter-progress', (event) => {
      const progress = event.payload;
      setFilterProgress(progress);

      // Track completed phases
      if (progress.operation !== 'complete') {
        const phaseIndex = FILTER_PHASES.findIndex(p => p.id === progress.operation);
        if (phaseIndex > 0) {
          const completed = FILTER_PHASES.slice(0, phaseIndex).map(p => p.id);
          setCompletedPhases(completed);
        }
      } else {
        setCompletedPhases(FILTER_PHASES.map(p => p.id));
        props.onComplete?.();
      }
    });
  });

  onCleanup(() => {
    if (timerInterval) clearInterval(timerInterval);
    unlistenScan?.();
    unlistenFilter?.();
  });

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const phases = () => props.mode === 'scan' ? SCAN_PHASES : FILTER_PHASES;

  const currentPhase = () => {
    if (props.mode === 'scan') {
      return scanProgress()?.phase || 'listing';
    }
    return filterProgress()?.operation || 'fetching_existing';
  };

  const progress = () => {
    if (props.mode === 'scan') {
      const sp = scanProgress();
      return { current: sp?.current || 0, total: sp?.total || 0, message: sp?.message };
    }
    const fp = filterProgress();
    return { current: fp?.current || 0, total: fp?.total || 0, message: fp?.filter_name };
  };

  const stats = (): Stat[] => {
    const p = progress();
    const timeStr = formatTime(elapsedTime());

    if (props.mode === 'scan') {
      return [
        { id: 'processed', label: 'Processed', value: p.current, variant: 'primary' },
        { id: 'total', label: 'Total', value: p.total },
        { id: 'elapsed', label: 'Elapsed', value: timeStr },
        { id: 'rate', label: 'Rate', value: elapsedTime() > 0 ? `${Math.round(p.current / elapsedTime())}/s` : '-' },
      ];
    }

    return [
      { id: 'processed', label: 'Processed', value: p.current, variant: 'primary' },
      { id: 'total', label: 'Total', value: p.total },
      { id: 'elapsed', label: 'Elapsed', value: timeStr },
    ];
  };

  const phaseLabel = () => {
    const phase = phases().find(p => p.id === currentPhase());
    return phase?.description || phase?.label || 'Processing...';
  };

  return (
    <div class="card p-6 space-y-6" role="region" aria-label="Operation progress">
      {/* Phase Indicator */}
      <PhaseIndicator
        phases={phases()}
        currentPhase={currentPhase()}
        completedPhases={completedPhases()}
      />

      {/* Main Progress */}
      <div class="space-y-4">
        <div class="text-center">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
            {phaseLabel()}
          </h3>
          <Show when={progress().message}>
            <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
              {progress().message}
            </p>
          </Show>
        </div>

        <ProgressBar
          current={progress().current}
          total={progress().total}
          showPercentage
          animated={currentPhase() !== 'complete'}
          size="lg"
        />
      </div>

      {/* Live Stats */}
      <LiveStats stats={stats()} size="md" animated={currentPhase() !== 'complete'} />

      {/* Cancel Button */}
      <Show when={props.onCancel && currentPhase() !== 'complete'}>
        <div class="flex justify-center">
          <button
            onClick={props.onCancel}
            class="btn-ghost text-red-600 hover:text-red-700 dark:text-red-400"
          >
            Cancel Operation
          </button>
        </div>
      </Show>

      {/* Screen reader announcement */}
      <div class="sr-only" aria-live="polite" aria-atomic="true">
        {phaseLabel()}: {progress().current} of {progress().total} processed
      </div>
    </div>
  );
};

export default ProgressView;
