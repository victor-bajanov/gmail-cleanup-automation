import { Component, For } from 'solid-js';

export interface Phase {
  id: string;
  label: string;
  description?: string;
}

export interface PhaseIndicatorProps {
  phases: Phase[];
  currentPhase: string;
  completedPhases: string[];
}

const PhaseIndicator: Component<PhaseIndicatorProps> = (props) => {
  const getPhaseStatus = (phaseId: string) => {
    if (props.completedPhases.includes(phaseId)) return 'completed';
    if (props.currentPhase === phaseId) return 'current';
    return 'pending';
  };

  const getPhaseIndex = (phaseId: string) => {
    return props.phases.findIndex(p => p.id === phaseId);
  };

  return (
    <nav aria-label="Progress phases">
      <ol class="flex items-center w-full" role="list">
        <For each={props.phases}>
          {(phase, index) => {
            const status = () => getPhaseStatus(phase.id);
            const isLast = () => index() === props.phases.length - 1;

            return (
              <li
                class={`flex items-center ${isLast() ? '' : 'flex-1'}`}
                aria-current={status() === 'current' ? 'step' : undefined}
              >
                <div class="flex flex-col items-center">
                  {/* Circle indicator */}
                  <div
                    class={`
                      w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium
                      transition-colors duration-200
                      ${status() === 'completed'
                        ? 'bg-green-600 text-white'
                        : status() === 'current'
                          ? 'bg-primary-600 text-white ring-4 ring-primary-100 dark:ring-primary-900'
                          : 'bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400'
                      }
                    `}
                    aria-hidden="true"
                  >
                    {status() === 'completed' ? (
                      <svg class="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
                        <path
                          fill-rule="evenodd"
                          d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                          clip-rule="evenodd"
                        />
                      </svg>
                    ) : (
                      index() + 1
                    )}
                  </div>

                  {/* Label */}
                  <span
                    class={`
                      mt-2 text-xs font-medium text-center max-w-[80px]
                      ${status() === 'completed'
                        ? 'text-green-600 dark:text-green-400'
                        : status() === 'current'
                          ? 'text-primary-600 dark:text-primary-400'
                          : 'text-gray-500 dark:text-gray-400'
                      }
                    `}
                  >
                    {phase.label}
                  </span>
                </div>

                {/* Connector line */}
                {!isLast() && (
                  <div
                    class={`
                      flex-1 h-0.5 mx-2 mt-[-1rem]
                      ${getPhaseIndex(props.currentPhase) > index() || props.completedPhases.includes(phase.id)
                        ? 'bg-green-600'
                        : 'bg-gray-200 dark:bg-gray-700'
                      }
                    `}
                    aria-hidden="true"
                  />
                )}
              </li>
            );
          }}
        </For>
      </ol>
    </nav>
  );
};

export default PhaseIndicator;
