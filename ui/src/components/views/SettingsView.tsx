import { Component, createSignal, onMount, Show, For } from 'solid-js';
import { ui } from '../../stores/app';
import * as api from '../../lib/api';
import type { AppSettings, ConfigSettings } from '../../types';

const SettingsView: Component = () => {
  // App settings state (GUI-only)
  const [appSettings, setAppSettings] = createSignal<AppSettings | null>(null);
  // Config settings state (config.toml)
  const [configSettings, setConfigSettings] = createSignal<ConfigSettings | null>(null);

  const [isLoading, setIsLoading] = createSignal(true);
  const [isSavingApp, setIsSavingApp] = createSignal(false);
  const [isSavingConfig, setIsSavingConfig] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [saveMessage, setSaveMessage] = createSignal<string | null>(null);

  // App settings form state
  const [theme, setTheme] = createSignal<'light' | 'dark' | 'system'>('system');
  const [showShortcuts, setShowShortcuts] = createSignal(true);
  const [soundEnabled, setSoundEnabled] = createSignal(false);
  const [autoAdvance, setAutoAdvance] = createSignal(true);

  // Config settings form state
  const [scanPeriodDays, setScanPeriodDays] = createSignal(30);
  const [maxConcurrentRequests, setMaxConcurrentRequests] = createSignal(10);
  const [classificationMode, setClassificationMode] = createSignal('rules');
  const [llmProvider, setLlmProvider] = createSignal('openai');
  const [minimumEmailsForLabel, setMinimumEmailsForLabel] = createSignal(5);
  const [labelPrefix, setLabelPrefix] = createSignal('');
  const [autoArchiveCategories, setAutoArchiveCategories] = createSignal('');
  const [dryRun, setDryRun] = createSignal(false);
  const [circuitBreakerEnabled, setCircuitBreakerEnabled] = createSignal(true);
  const [failureThreshold, setFailureThreshold] = createSignal(5);
  const [resetTimeoutSecs, setResetTimeoutSecs] = createSignal(60);

  onMount(async () => {
    await loadAllSettings();
  });

  const loadAllSettings = async () => {
    setIsLoading(true);
    setError(null);

    try {
      // Load both settings in parallel
      const [appLoaded, configLoaded] = await Promise.all([
        api.getSettings(),
        api.getConfigSettings().catch(() => null), // Config settings may not exist yet
      ]);

      // Update app settings state
      setAppSettings(appLoaded);
      setTheme(appLoaded.theme as 'light' | 'dark' | 'system');
      setShowShortcuts(appLoaded.show_shortcuts);
      setSoundEnabled(appLoaded.sound_enabled);
      setAutoAdvance(appLoaded.auto_advance);

      // Update config settings state if loaded
      if (configLoaded) {
        setConfigSettings(configLoaded);
        setScanPeriodDays(configLoaded.scan_period_days);
        setMaxConcurrentRequests(configLoaded.max_concurrent_requests);
        setClassificationMode(configLoaded.classification_mode);
        setLlmProvider(configLoaded.llm_provider);
        setMinimumEmailsForLabel(configLoaded.minimum_emails_for_label);
        setLabelPrefix(configLoaded.label_prefix);
        setAutoArchiveCategories(configLoaded.auto_archive_categories.join(', '));
        setDryRun(configLoaded.dry_run);
        setCircuitBreakerEnabled(configLoaded.circuit_breaker_enabled);
        setFailureThreshold(configLoaded.failure_threshold);
        setResetTimeoutSecs(configLoaded.reset_timeout_secs);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsLoading(false);
    }
  };

  const handleSaveAppSettings = async () => {
    setIsSavingApp(true);
    setError(null);
    setSaveMessage(null);

    try {
      const currentApp = appSettings();
      if (!currentApp) return;

      const newSettings: AppSettings = {
        ...currentApp,
        theme: theme(),
        show_shortcuts: showShortcuts(),
        sound_enabled: soundEnabled(),
        auto_advance: autoAdvance(),
      };

      await api.saveSettings(newSettings);
      setAppSettings(newSettings);

      // Apply theme immediately
      applyTheme(theme());

      setSaveMessage('Application settings saved successfully');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSavingApp(false);
    }
  };

  const handleSaveConfigSettings = async () => {
    setIsSavingConfig(true);
    setError(null);
    setSaveMessage(null);

    try {
      const newConfigSettings: ConfigSettings = {
        scan_period_days: scanPeriodDays(),
        max_concurrent_requests: maxConcurrentRequests(),
        classification_mode: classificationMode(),
        llm_provider: llmProvider(),
        minimum_emails_for_label: minimumEmailsForLabel(),
        label_prefix: labelPrefix(),
        auto_archive_categories: autoArchiveCategories()
          .split(',')
          .map((s) => s.trim())
          .filter((s) => s.length > 0),
        dry_run: dryRun(),
        circuit_breaker_enabled: circuitBreakerEnabled(),
        failure_threshold: failureThreshold(),
        reset_timeout_secs: resetTimeoutSecs(),
      };

      await api.saveConfigSettings(newConfigSettings);
      setConfigSettings(newConfigSettings);

      setSaveMessage('Configuration settings saved successfully');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSavingConfig(false);
    }
  };

  const handleResetAppSettings = async () => {
    setIsSavingApp(true);
    setError(null);
    setSaveMessage(null);

    try {
      const defaultSettings = await api.resetSettings();
      setAppSettings(defaultSettings);

      setTheme(defaultSettings.theme as 'light' | 'dark' | 'system');
      setShowShortcuts(defaultSettings.show_shortcuts);
      setSoundEnabled(defaultSettings.sound_enabled);
      setAutoAdvance(defaultSettings.auto_advance);

      applyTheme(defaultSettings.theme as 'light' | 'dark' | 'system');

      setSaveMessage('Application settings reset to defaults');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSavingApp(false);
    }
  };

  const handleResetConfigSettings = async () => {
    setIsSavingConfig(true);
    setError(null);
    setSaveMessage(null);

    try {
      const defaultConfig = await api.resetConfigSettings();
      setConfigSettings(defaultConfig);

      setScanPeriodDays(defaultConfig.scan_period_days);
      setMaxConcurrentRequests(defaultConfig.max_concurrent_requests);
      setClassificationMode(defaultConfig.classification_mode);
      setLlmProvider(defaultConfig.llm_provider);
      setMinimumEmailsForLabel(defaultConfig.minimum_emails_for_label);
      setLabelPrefix(defaultConfig.label_prefix);
      setAutoArchiveCategories(defaultConfig.auto_archive_categories.join(', '));
      setDryRun(defaultConfig.dry_run);
      setCircuitBreakerEnabled(defaultConfig.circuit_breaker_enabled);
      setFailureThreshold(defaultConfig.failure_threshold);
      setResetTimeoutSecs(defaultConfig.reset_timeout_secs);

      setSaveMessage('Configuration settings reset to defaults');
      setTimeout(() => setSaveMessage(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIsSavingConfig(false);
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

  // Toggle switch component
  const Toggle: Component<{ id: string; checked: boolean; onChange: () => void; label: string; description: string }> = (props) => (
    <div class="flex items-center justify-between">
      <div>
        <label for={props.id} class="text-sm font-medium text-gray-700 dark:text-gray-300">
          {props.label}
        </label>
        <p class="text-xs text-gray-500 dark:text-gray-400">{props.description}</p>
      </div>
      <button
        id={props.id}
        role="switch"
        aria-checked={props.checked}
        onClick={props.onChange}
        class={`
          relative inline-flex h-6 w-11 items-center rounded-full transition-colors
          ${props.checked ? 'bg-primary-600' : 'bg-gray-200 dark:bg-gray-700'}
        `}
      >
        <span
          class={`
            inline-block h-4 w-4 transform rounded-full bg-white transition-transform
            ${props.checked ? 'translate-x-6' : 'translate-x-1'}
          `}
        />
      </button>
    </div>
  );

  return (
    <div class="max-w-2xl mx-auto space-y-6">
      {/* Header */}
      <div>
        <h2 class="text-xl font-bold text-gray-900 dark:text-white">Settings</h2>
        <p class="text-gray-500 dark:text-gray-400">Configure application preferences and configuration</p>
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
        {/* ==================== APPLICATION SETTINGS (GUI-only) ==================== */}
        <div class="space-y-4">
          <div class="border-b border-gray-200 dark:border-gray-700 pb-2">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Application Settings</h3>
            <p class="text-sm text-gray-500 dark:text-gray-400">GUI preferences (stored locally)</p>
          </div>

          {/* Appearance */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Appearance</h4>

            <div class="space-y-4">
              <div>
                <label for="theme" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
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

              <Toggle
                id="show-shortcuts"
                checked={showShortcuts()}
                onChange={() => setShowShortcuts(!showShortcuts())}
                label="Show Keyboard Shortcuts"
                description="Display shortcut hints in the UI"
              />
            </div>
          </div>

          {/* Behavior */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Behavior</h4>

            <div class="space-y-4">
              <Toggle
                id="auto-advance"
                checked={autoAdvance()}
                onChange={() => setAutoAdvance(!autoAdvance())}
                label="Auto-Advance"
                description="Automatically move to next cluster after decision"
              />

              <Toggle
                id="sound-enabled"
                checked={soundEnabled()}
                onChange={() => setSoundEnabled(!soundEnabled())}
                label="Sound Effects"
                description="Play sounds for notifications"
              />
            </div>
          </div>

          {/* App Settings Actions */}
          <div class="flex justify-between">
            <button
              onClick={handleResetAppSettings}
              disabled={isSavingApp()}
              class="btn-ghost text-red-600 hover:text-red-700 dark:text-red-400"
            >
              Reset App Settings
            </button>

            <button onClick={handleSaveAppSettings} disabled={isSavingApp()} class="btn-primary">
              {isSavingApp() ? 'Saving...' : 'Save App Settings'}
            </button>
          </div>
        </div>

        {/* ==================== CONFIGURATION SETTINGS (config.toml) ==================== */}
        <div class="space-y-4 mt-8">
          <div class="border-b border-gray-200 dark:border-gray-700 pb-2">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Configuration Settings</h3>
            <p class="text-sm text-gray-500 dark:text-gray-400">Backend configuration (config.toml)</p>
          </div>

          {/* Scan Settings */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Scan Settings</h4>

            <div class="space-y-4">
              <div>
                <label for="scan-period" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Scan Period (days)
                </label>
                <input
                  id="scan-period"
                  type="number"
                  value={scanPeriodDays()}
                  onInput={(e) => setScanPeriodDays(Math.max(1, Math.min(365, parseInt(e.currentTarget.value) || 30)))}
                  min="1"
                  max="365"
                  class="input w-full"
                  aria-describedby="scan-period-help"
                />
                <p id="scan-period-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Number of days to scan when analyzing emails (1-365)
                </p>
              </div>

              <div>
                <label for="max-concurrent" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Max Concurrent Requests
                </label>
                <input
                  id="max-concurrent"
                  type="number"
                  value={maxConcurrentRequests()}
                  onInput={(e) => setMaxConcurrentRequests(Math.max(1, Math.min(50, parseInt(e.currentTarget.value) || 10)))}
                  min="1"
                  max="50"
                  class="input w-full"
                  aria-describedby="max-concurrent-help"
                />
                <p id="max-concurrent-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Maximum parallel API requests (1-50)
                </p>
              </div>
            </div>
          </div>

          {/* Classification Settings */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Classification</h4>

            <div class="space-y-4">
              <div>
                <label for="classification-mode" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Classification Mode
                </label>
                <select
                  id="classification-mode"
                  value={classificationMode()}
                  onChange={(e) => setClassificationMode(e.currentTarget.value)}
                  class="input w-full"
                >
                  <option value="rules">Rules</option>
                  <option value="ml">Machine Learning</option>
                  <option value="hybrid">Hybrid</option>
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Method used to classify emails
                </p>
              </div>

              <div>
                <label for="llm-provider" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  LLM Provider
                </label>
                <select
                  id="llm-provider"
                  value={llmProvider()}
                  onChange={(e) => setLlmProvider(e.currentTarget.value)}
                  class="input w-full"
                >
                  <option value="openai">OpenAI</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="anthropic-agents">Anthropic Agents</option>
                </select>
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Provider for ML-based classification
                </p>
              </div>

              <div>
                <label for="min-emails-label" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Minimum Emails for Label
                </label>
                <input
                  id="min-emails-label"
                  type="number"
                  value={minimumEmailsForLabel()}
                  onInput={(e) => setMinimumEmailsForLabel(Math.max(1, Math.min(100, parseInt(e.currentTarget.value) || 5)))}
                  min="1"
                  max="100"
                  class="input w-full"
                  aria-describedby="min-emails-help"
                />
                <p id="min-emails-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Minimum emails required to create a label (1-100)
                </p>
              </div>
            </div>
          </div>

          {/* Labels Settings */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Labels</h4>

            <div class="space-y-4">
              <div>
                <label for="label-prefix" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
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

              <div>
                <label for="auto-archive" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                  Auto-Archive Categories
                </label>
                <input
                  id="auto-archive"
                  type="text"
                  value={autoArchiveCategories()}
                  onInput={(e) => setAutoArchiveCategories(e.currentTarget.value)}
                  placeholder="e.g., newsletters, promotions, notifications"
                  class="input w-full"
                  aria-describedby="auto-archive-help"
                />
                <p id="auto-archive-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                  Comma-separated list of categories to auto-archive
                </p>
              </div>
            </div>
          </div>

          {/* Execution Settings */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Execution</h4>

            <div class="space-y-4">
              <Toggle
                id="dry-run"
                checked={dryRun()}
                onChange={() => setDryRun(!dryRun())}
                label="Dry Run Mode"
                description="Preview changes without actually applying them"
              />
            </div>
          </div>

          {/* Circuit Breaker Settings */}
          <div class="card p-6">
            <h4 class="text-md font-semibold text-gray-900 dark:text-white mb-4">Circuit Breaker</h4>

            <div class="space-y-4">
              <Toggle
                id="circuit-breaker"
                checked={circuitBreakerEnabled()}
                onChange={() => setCircuitBreakerEnabled(!circuitBreakerEnabled())}
                label="Enable Circuit Breaker"
                description="Automatically pause operations after repeated failures"
              />

              <Show when={circuitBreakerEnabled()}>
                <div>
                  <label for="failure-threshold" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                    Failure Threshold
                  </label>
                  <input
                    id="failure-threshold"
                    type="number"
                    value={failureThreshold()}
                    onInput={(e) => setFailureThreshold(Math.max(1, Math.min(20, parseInt(e.currentTarget.value) || 5)))}
                    min="1"
                    max="20"
                    class="input w-full"
                    aria-describedby="failure-threshold-help"
                  />
                  <p id="failure-threshold-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Number of failures before circuit opens (1-20)
                  </p>
                </div>

                <div>
                  <label for="reset-timeout" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                    Reset Timeout (seconds)
                  </label>
                  <input
                    id="reset-timeout"
                    type="number"
                    value={resetTimeoutSecs()}
                    onInput={(e) => setResetTimeoutSecs(Math.max(10, Math.min(300, parseInt(e.currentTarget.value) || 60)))}
                    min="10"
                    max="300"
                    class="input w-full"
                    aria-describedby="reset-timeout-help"
                  />
                  <p id="reset-timeout-help" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Seconds before attempting to close circuit (10-300)
                  </p>
                </div>
              </Show>
            </div>
          </div>

          {/* Config Settings Actions */}
          <div class="flex justify-between">
            <button
              onClick={handleResetConfigSettings}
              disabled={isSavingConfig()}
              class="btn-ghost text-red-600 hover:text-red-700 dark:text-red-400"
            >
              Reset Config Settings
            </button>

            <button onClick={handleSaveConfigSettings} disabled={isSavingConfig()} class="btn-primary">
              {isSavingConfig() ? 'Saving...' : 'Save Config Settings'}
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default SettingsView;
