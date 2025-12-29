import { Component } from 'solid-js';
import type { ErrorEvent } from '../../types';

interface Props {
  error: ErrorEvent;
  onDismiss: () => void;
}

const ErrorToast: Component<Props> = (props) => {
  return (
    <div class="fixed bottom-4 right-4 z-50 animate-slide-in">
      <div
        class="max-w-md p-4 rounded-lg shadow-lg border"
        classList={{
          'bg-red-50 dark:bg-red-900/30 border-red-200 dark:border-red-800': !props.error.recoverable,
          'bg-yellow-50 dark:bg-yellow-900/30 border-yellow-200 dark:border-yellow-800': props.error.recoverable,
        }}
      >
        <div class="flex items-start gap-3">
          {/* Icon */}
          <div
            class="flex-shrink-0 w-5 h-5 rounded-full flex items-center justify-center"
            classList={{
              'bg-red-100 dark:bg-red-800 text-red-600 dark:text-red-300': !props.error.recoverable,
              'bg-yellow-100 dark:bg-yellow-800 text-yellow-600 dark:text-yellow-300': props.error.recoverable,
            }}
          >
            {props.error.recoverable ? '⚠' : '✕'}
          </div>

          {/* Content */}
          <div class="flex-1 min-w-0">
            <p
              class="text-sm font-medium"
              classList={{
                'text-red-800 dark:text-red-200': !props.error.recoverable,
                'text-yellow-800 dark:text-yellow-200': props.error.recoverable,
              }}
            >
              {props.error.code}
            </p>
            <p
              class="mt-1 text-sm"
              classList={{
                'text-red-700 dark:text-red-300': !props.error.recoverable,
                'text-yellow-700 dark:text-yellow-300': props.error.recoverable,
              }}
            >
              {props.error.message}
            </p>
          </div>

          {/* Dismiss button */}
          <button
            onClick={() => props.onDismiss()}
            class="flex-shrink-0 p-1 rounded hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
            aria-label="Dismiss"
          >
            <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
              <path
                fill-rule="evenodd"
                d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                clip-rule="evenodd"
              />
            </svg>
          </button>
        </div>
      </div>
    </div>
  );
};

export default ErrorToast;
