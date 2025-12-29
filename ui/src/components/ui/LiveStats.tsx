import { Component, For, Show } from 'solid-js';

export interface Stat {
  id: string;
  label: string;
  value: number | string;
  icon?: string;
  change?: number;
  variant?: 'default' | 'success' | 'warning' | 'error' | 'primary';
}

export interface LiveStatsProps {
  stats: Stat[];
  layout?: 'row' | 'grid';
  size?: 'sm' | 'md' | 'lg';
  animated?: boolean;
}

const LiveStats: Component<LiveStatsProps> = (props) => {
  const variantClasses = (variant?: string) => {
    switch (variant) {
      case 'success':
        return 'text-green-600 dark:text-green-400';
      case 'warning':
        return 'text-yellow-600 dark:text-yellow-400';
      case 'error':
        return 'text-red-600 dark:text-red-400';
      case 'primary':
        return 'text-primary-600 dark:text-primary-400';
      default:
        return 'text-gray-900 dark:text-white';
    }
  };

  const sizeClasses = () => {
    switch (props.size) {
      case 'sm':
        return { value: 'text-lg', label: 'text-xs' };
      case 'lg':
        return { value: 'text-4xl', label: 'text-base' };
      default:
        return { value: 'text-2xl', label: 'text-sm' };
    }
  };

  const layoutClasses = () => {
    if (props.layout === 'row') {
      return 'flex flex-wrap gap-6';
    }
    return `grid gap-4 ${props.stats.length <= 2 ? 'grid-cols-2' : props.stats.length <= 4 ? 'grid-cols-4' : 'grid-cols-3 md:grid-cols-5'}`;
  };

  return (
    <div
      class={layoutClasses()}
      role="region"
      aria-label="Live statistics"
      aria-live="polite"
    >
      <For each={props.stats}>
        {(stat) => (
          <div
            class={`
              ${props.layout === 'row' ? '' : 'p-4 bg-gray-50 dark:bg-gray-700 rounded-lg'}
              ${props.animated ? 'transition-all duration-300' : ''}
            `}
          >
            <div class={`${props.layout === 'row' ? '' : 'text-center'}`}>
              <Show when={stat.icon}>
                <span class="text-xl mb-1 block" aria-hidden="true">
                  {stat.icon}
                </span>
              </Show>
              <div
                class={`font-bold ${sizeClasses().value} ${variantClasses(stat.variant)} ${props.animated ? 'animate-pulse' : ''}`}
                aria-label={`${stat.label}: ${stat.value}`}
              >
                {typeof stat.value === 'number' ? stat.value.toLocaleString() : stat.value}
                <Show when={stat.change !== undefined}>
                  <span
                    class={`ml-2 text-sm ${stat.change! > 0 ? 'text-green-500' : stat.change! < 0 ? 'text-red-500' : 'text-gray-400'}`}
                  >
                    {stat.change! > 0 ? '+' : ''}{stat.change}
                  </span>
                </Show>
              </div>
              <div class={`${sizeClasses().label} text-gray-500 dark:text-gray-400`}>
                {stat.label}
              </div>
            </div>
          </div>
        )}
      </For>
    </div>
  );
};

export default LiveStats;
