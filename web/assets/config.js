/**
 * Runtime configuration for the OpenVM UPLC Verifier.
 *
 * Can be customized via:
 * 1. Environment variables at build time (NODE_ENV, BACKEND_URL)
 * 2. Global window.CONFIG object (runtime override)
 * 3. URL query parameters (e.g., ?backendUrl=http://localhost:8080)
 */

// Default backend URL — use relative path for same-origin deployment
const DEFAULT_BACKEND_URL = '/';

function getBackendUrl() {
  // 1. Check URL query parameter (?backendUrl=...)
  const params = new URLSearchParams(window.location.search);
  if (params.has('backendUrl')) {
    return params.get('backendUrl');
  }

  // 2. Check global window.CONFIG.backendUrl
  if (window.CONFIG?.backendUrl) {
    return window.CONFIG.backendUrl;
  }

  // 3. Check environment variable (injected at build time via esbuild define)
  if (typeof BACKEND_URL_CONFIG !== 'undefined') {
    return BACKEND_URL_CONFIG;
  }

  // 4. Use default
  return DEFAULT_BACKEND_URL;
}

export const config = {
  backendUrl: getBackendUrl(),

  /**
   * Get the full API endpoint URL
   */
  apiUrl(path) {
    const base = this.backendUrl.replace(/\/$/, ''); // remove trailing slash
    return base + path;
  },
};

console.log('[config] Backend URL:', config.backendUrl);
