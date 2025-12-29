import { Component, Show } from 'solid-js';

export interface ProgressBarProps {
  current: number;
  total: number;
  label?: string;
  showPercentage?: boolean;
  variant?: 'default' | 'success' | 'warning' | 'error';
  size?: 'sm' | 'md' | 'lg';
  animated?: boolean;
}

const ProgressBar: Component<ProgressBarProps> = (props) => {
  const percentage = () => {
    if (props.total === 0) return 0;
    return Math.min(100, Math.round((props.current / props.total) * 100));
  };

  const variantClasses = () => {
    switch (props.variant) {
      case 'success':
        return 'bg-green-600';
      case 'warning':
        return 'bg-yellow-500';
      case 'error':
        return 'bg-red-600';
      default:
        return 'bg-primary-600';
    }
  };

  const sizeClasses = () => {
    switch (props.size) {
      case 'sm':
        return 'h-1';
      case 'lg':
        return 'h-4';
      default:
        return 'h-2';
    }
  };

  return (
    <div
      class="w-full"
      role="progressbar"
      aria-valuenow={props.current}
      aria-valuemin={0}
      aria-valuemax={props.total}
      aria-label={props.label || 'Progress'}
    >
      <Show when={props.label || props.showPercentage}>
        <div class="flex justify-between mb-1 text-sm">
          <Show when={props.label}>
            <span class="text-gray-700 dark:text-gray-300">{props.label}</span>
          </Show>
          <Show when={props.showPercentage}>
            <span class="text-gray-500 dark:text-gray-400">
              {percentage()}%
            </span>
          </Show>
        </div>
      </Show>
      <div class={`bg-gray-200 dark:bg-gray-700 rounded-full overflow-hidden ${sizeClasses()}`}>
        <div
          class={`${sizeClasses()} ${variantClasses()} rounded-full transition-all duration-300 ${props.animated ? 'animate-pulse' : ''}`}
          style={{ width: `${percentage()}%` }}
        />
      </div>
    </div>
  );
};

export default ProgressBar;
