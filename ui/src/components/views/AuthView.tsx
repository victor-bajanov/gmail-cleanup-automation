import { Component, createSignal, Show } from 'solid-js';
import { auth, navigation } from '../../stores/app';
import * as api from '../../lib/api';

const AuthView: Component = () => {
  const [error, setError] = createSignal<string | null>(null);

  const handleAuthenticate = async () => {
    setError(null);
    auth.setAuthenticating(true);

    try {
      const status = await api.authenticate();
      auth.setStatus(status);

      if (status.authenticated) {
        await api.initializeClient();
        navigation.goTo('scan');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      auth.setAuthenticating(false);
    }
  };

  return (
    <div class="max-w-md mx-auto mt-20">
      <div class="card p-8">
        <div class="text-center mb-8">
          <div class="text-6xl mb-4">📧</div>
          <h1 class="text-2xl font-bold text-gray-900 dark:text-white">
            Gmail Cleanup
          </h1>
          <p class="mt-2 text-gray-600 dark:text-gray-400">
            Sign in with your Google account to manage your email filters
          </p>
        </div>

        <Show when={error()}>
          <div class="mb-6 p-4 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg">
            <p class="text-sm text-red-700 dark:text-red-300">{error()}</p>
          </div>
        </Show>

        <Show when={auth.status()}>
          <div class="mb-6 p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
            <div class="text-sm space-y-2">
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Credentials:</span>
                <span
                  classList={{
                    'text-green-600 dark:text-green-400': auth.status()!.credentials_exist,
                    'text-red-600 dark:text-red-400': !auth.status()!.credentials_exist,
                  }}
                >
                  {auth.status()!.credentials_exist ? 'Found' : 'Not found'}
                </span>
              </div>
              <div class="flex justify-between">
                <span class="text-gray-500 dark:text-gray-400">Token:</span>
                <span
                  classList={{
                    'text-green-600 dark:text-green-400': auth.status()!.token_exists,
                    'text-yellow-600 dark:text-yellow-400': !auth.status()!.token_exists,
                  }}
                >
                  {auth.status()!.token_exists ? 'Cached' : 'Not cached'}
                </span>
              </div>
              <Show when={auth.status()!.email}>
                <div class="flex justify-between">
                  <span class="text-gray-500 dark:text-gray-400">Account:</span>
                  <span class="text-gray-900 dark:text-white font-medium">
                    {auth.status()!.email}
                  </span>
                </div>
              </Show>
            </div>
          </div>
        </Show>

        <button
          onClick={handleAuthenticate}
          disabled={auth.isAuthenticating()}
          class="btn-primary w-full flex items-center justify-center gap-2"
        >
          <Show
            when={!auth.isAuthenticating()}
            fallback={
              <>
                <svg
                  class="animate-spin h-5 w-5"
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                >
                  <circle
                    class="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    stroke-width="4"
                  />
                  <path
                    class="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                  />
                </svg>
                <span>Authenticating...</span>
              </>
            }
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24">
              <path
                fill="currentColor"
                d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"
              />
              <path
                fill="currentColor"
                d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
              />
              <path
                fill="currentColor"
                d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"
              />
              <path
                fill="currentColor"
                d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"
              />
            </svg>
            <span>Sign in with Google</span>
          </Show>
        </button>

        <p class="mt-6 text-xs text-center text-gray-500 dark:text-gray-400">
          Make sure you have <code class="font-mono bg-gray-100 dark:bg-gray-700 px-1 rounded">credentials.json</code> in{' '}
          <code class="font-mono bg-gray-100 dark:bg-gray-700 px-1 rounded">~/.gmail-automation/</code>
        </p>
      </div>
    </div>
  );
};

export default AuthView;
