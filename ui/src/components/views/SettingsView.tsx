import { Component, createSignal, onMount, Show } from 'solid-js';
import { ui } from '../../stores/app';
import * as api from '../../lib/api';
import type { AppSettings } from '../../types';

const SettingsView: Component = () => {
  const [settings, setSettings] = createSignal<AppSettings | null>(null);
  const [isLoading, setIsLoading] = createSignal(true);
  const [isSaving, setIsSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [saveMessage, setSaveMessage] = createSignal<string | null>(null);

  // Local form state
  const [scanPeriodDays, setScanPeriodDays] = createSignal(30);
  const [minClusterSize, setMinClusterSize] = createSignal(3);
  const [labelPrefix, setLabelPrefix] = createSignal('');
  const [defaultArchive, setDefaultArchive] = createSignal(true);
  const [theme, setTheme] = createSignal<'light' | 'dark' | 'system'>('system');
  const [showShortcuts, setShowShortcuts] = createSignal(true);
  const [soundEnabled, setSoundEnabled] = createSignal(false);
  const [autoAdvance, setAutoAdvance] = createSignal(true);

  onMount(async () => {
    await loadSettings();
  });

  const loadSettings = async () => {
    setIsLoading(true);
    setError(null);

    try {
      const loaded = await api.getSettings();
      setSettings(loaded);

      // Update local state
      setScanPeriodDays(loaded.scan_period_days);
      setMinClusterSize(loaded.min_cluster_size);
      setLabelPrefix(loaded.label_prefix);
      setDefaultArchive(loaded.default_archive);
      setTheme(loaded.theme as 'light' | 'dark' | 'system');
      setShowShortcuts(loaded.show_shortcuts);
      setSoundEnabled(loaded.sound_enabled);
      setAutoAdvance(loaded.auto_advance);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const handleSave = async () => {
    setIsSaving(true);
    setError(null);
    setSaveMessage(null);

    try {
      const newSettings: AppSettings = {
        scan_period_days: scanPeriodDays(),
        min_cluster_size: minClusterSize(),
        label_prefix: labelPrefix(),
        default_archive: defaultArchive(),
        theme: theme(),
        show_shortcuts: showShortcuts(),
        sound_enabled: soundEnabled(),
        auto_advance: autoAdvance(),
      };

      await api.saveSettings(newSettings);
      setSettings(newSettings);

      // Apply theme immediately
      applyTheme(theme());

      setSaveMessage('Settings saved successfully');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const handleReset = async () => {
    setIsSaving(true);
    setError(null);
    setSaveMessage(null);

    try {
      const defaultSettings = await api.resetSettings();
      setSettings(defaultSettings);

      // Update local state
      setScanPeriodDays(defaultSettings.scan_period_days);
      setMinClusterSize(defaultSettings.min_cluster_size);
      setLabelPrefix(defaultSettings.label_prefix);
      setDefaultArchive(defaultSettings.default_archive);
      setTheme(defaultSettings.theme as 'light' | 'dark' | 'system');
      setShowShortcuts(defaultSettings.show_shortcuts);
      setSoundEnabled(defaultSettings.sound_enabled);
      setAutoAdvance(defaultSettings.auto_advance);

      applyTheme(defaultSettings.theme as 'light' | 'dark' | 'system');

      setSaveMessage('Settings reset to defaults');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSaving(false);
    }
  };

  const applyTheme = (newTheme: 'light' | 'dark' | 'system') => {
    if (newTheme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      document.documentElement.classList.toggle('dark', prefersDark);
    } else {
      document.documentElement.classList.toggle('dark', newTheme === 'dark');
    }
    ui.setDarkMode(newTheme === 'dark' || (newTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches));
  };

  return (
    <div class="max-w-2xl mx-auto space-y-6">
      {/* Header */}
      <div>
        <h2 class="text-xl font-bold text-gray-900 dark:text-white">
          Settings
        </h2>
        <p class="text-gray-500 dark:text-gray-400">
          Configure application preferences
        </p>
      </div>

      {/* Loading */}
      <Show when={isLoading()}>
        <div class="card p-8 text-center">
          <div class="animate-spin w-8 h-8 border-4 border-primary-200 border-t-primary-600 rounded-full mx-auto mb-4" />
          <p class="text-gray-500 dark:text-gray-400">Loading settings...</p>
        </div>
      </Show>

      {/* Error */}
      <Show when={error()}>
        <div class="card p-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/30">
          <p class="text-red-700 dark:text-red-300">{error()}</p>
        </div>
      </Show>

      {/* Success Message */}
      <Show when={saveMessage()}>
        <div class="card p-4 border-green-200 dark:border-green-800 bg-green-50 dark:bg-green-900/30">
          <p class="text-green-700 dark:text-green-300">{saveMessage()}</p>
        </div>
      </Show>

      <Show when={!isLoading()}>
        {/* Scan Settings */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Scan Settings
          </h3>

          <div class="space-y-4">
            <div>
              <label
                for="scan-period"
                class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Default Scan Period (days)
              </label>
              <input
                id="scan-period"
                type="number"
                value={scanPeriodDays()}
                onInput={(e) => setScanPeriodDays(parseInt(e.currentTarget.value) || 30)}
                min="1"
                max="365"
                class="input w-full"
                aria-describedby="scan-period-help"
              />
              <p id="scan-period-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                Number of days to scan when analyzing emails
              </p>
            </div>

            <div>
              <label
                for="min-cluster"
                class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Minimum Cluster Size
              </label>
              <input
                id="min-cluster"
                type="number"
                value={minClusterSize()}
                onInput={(e) => setMinClusterSize(parseInt(e.currentTarget.value) || 3)}
                min="1"
                max="100"
                class="input w-full"
                aria-describedby="min-cluster-help"
              />
              <p id="min-cluster-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                Minimum emails required to form a cluster for review
              </p>
            </div>
          </div>
        </div>

        {/* Filter Settings */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Filter Settings
          </h3>

          <div class="space-y-4">
            <div>
              <label
                for="label-prefix"
                class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Label Prefix
              </label>
              <input
                id="label-prefix"
                type="text"
                value={labelPrefix()}
                onInput={(e) => setLabelPrefix(e.currentTarget.value)}
                placeholder="e.g., Auto/"
                class="input w-full"
                aria-describedby="label-prefix-help"
              />
              <p id="label-prefix-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                Prefix added to auto-generated label names (leave empty for no prefix)
              </p>
            </div>

            <div class="flex items-center justify-between">
              <div>
                <label
                  for="default-archive"
                  class="text-sm font-medium text-gray-700 dark:text-gray-300"
                >
                  Archive by Default
                </label>
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Skip inbox for new filters by default
                </p>
              </div>
              <button
                id="default-archive"
                role="switch"
                aria-checked={defaultArchive()}
                onClick={() => setDefaultArchive(!defaultArchive())}
                class={`
                  relative inline-flex h-6 w-11 items-center rounded-full transition-colors
                  ${defaultArchive() ? 'bg-primary-600' : 'bg-gray-200 dark:bg-gray-700'}
                `}
              >
                <span
                  class={`
                    inline-block h-4 w-4 transform rounded-full bg-white transition-transform
                    ${defaultArchive() ? 'translate-x-6' : 'translate-x-1'}
                  `}
                />
              </button>
            </div>
          </div>
        </div>

        {/* Appearance */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Appearance
          </h3>

          <div class="space-y-4">
            <div>
              <label
                for="theme"
                class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
              >
                Theme
              </label>
              <select
                id="theme"
                value={theme()}
                onChange={(e) => setTheme(e.currentTarget.value as 'light' | 'dark' | 'system')}
                class="input w-full"
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </div>

            <div class="flex items-center justify-between">
              <div>
                <label
                  for="show-shortcuts"
                  class="text-sm font-medium text-gray-700 dark:text-gray-300"
                >
                  Show Keyboard Shortcuts
                </label>
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Display shortcut hints in the UI
                </p>
              </div>
              <button
                id="show-shortcuts"
                role="switch"
                aria-checked={showShortcuts()}
                onClick={() => setShowShortcuts(!showShortcuts())}
                class={`
                  relative inline-flex h-6 w-11 items-center rounded-full transition-colors
                  ${showShortcuts() ? 'bg-primary-600' : 'bg-gray-200 dark:bg-gray-700'}
                `}
              >
                <span
                  class={`
                    inline-block h-4 w-4 transform rounded-full bg-white transition-transform
                    ${showShortcuts() ? 'translate-x-6' : 'translate-x-1'}
                  `}
                />
              </button>
            </div>
          </div>
        </div>

        {/* Behavior */}
        <div class="card p-6">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-4">
            Behavior
          </h3>

          <div class="space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <label
                  for="auto-advance"
                  class="text-sm font-medium text-gray-700 dark:text-gray-300"
                >
                  Auto-Advance
                </label>
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Automatically move to next cluster after decision
                </p>
              </div>
              <button
                id="auto-advance"
                role="switch"
                aria-checked={autoAdvance()}
                onClick={() => setAutoAdvance(!autoAdvance())}
                class={`
                  relative inline-flex h-6 w-11 items-center rounded-full transition-colors
                  ${autoAdvance() ? 'bg-primary-600' : 'bg-gray-200 dark:bg-gray-700'}
                `}
              >
                <span
                  class={`
                    inline-block h-4 w-4 transform rounded-full bg-white transition-transform
                    ${autoAdvance() ? 'translate-x-6' : 'translate-x-1'}
                  `}
                />
              </button>
            </div>

            <div class="flex items-center justify-between">
              <div>
                <label
                  for="sound-enabled"
                  class="text-sm font-medium text-gray-700 dark:text-gray-300"
                >
                  Sound Effects
                </label>
                <p class="text-xs text-gray-500 dark:text-gray-400">
                  Play sounds for notifications
                </p>
              </div>
              <button
                id="sound-enabled"
                role="switch"
                aria-checked={soundEnabled()}
                onClick={() => setSoundEnabled(!soundEnabled())}
                class={`
                  relative inline-flex h-6 w-11 items-center rounded-full transition-colors
                  ${soundEnabled() ? 'bg-primary-600' : 'bg-gray-200 dark:bg-gray-700'}
                `}
              >
                <span
                  class={`
                    inline-block h-4 w-4 transform rounded-full bg-white transition-transform
                    ${soundEnabled() ? 'translate-x-6' : 'translate-x-1'}
                  `}
                />
              </button>
            </div>
          </div>
        </div>

        {/* Actions */}
        <div class="flex justify-between">
          <button
            onClick={handleReset}
            disabled={isSaving()}
            class="btn-ghost text-red-600 hover:text-red-700 dark:text-red-400"
          >
            Reset to Defaults
          </button>

          <button
            onClick={handleSave}
            disabled={isSaving()}
            class="btn-primary"
          >
            {isSaving() ? 'Saving...' : 'Save Settings'}
          </button>
        </div>
      </Show>
    </div>
  );
};

export default SettingsView;
