/**
 * Injects a mock for window.__TAURI_INTERNALS__.invoke into the page.
 * Call via page.evaluateOnNewDocument() before navigation.
 *
 * mockResponses is a Record<commandName, responseData>.
 * Any command not in the map rejects with "unmocked command: <name>".
 */
export function buildMockScript(mockResponses: Record<string, unknown>): string {
  return `
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        const responses = ${JSON.stringify(mockResponses)};
        if (cmd in responses) {
          return responses[cmd];
        }
        throw new Error('unmocked command: ' + cmd);
      },
      convertFileSrc: (src) => src,
      transformCallback: (cb) => {
        const id = Math.random();
        window['_' + id] = cb;
        return id;
      },
    };
  `;
}
