/**
 * Client-side helpers for the web verifier UI.
 *
 * The browser still checks the program output and commitment locally. The
 * cryptographic proof itself is now verified by the backend's native OpenVM
 * verifier.
 */

export function normalizePublicValuesHex(hex) {
  return typeof hex === 'string' ? hex.replace(/^0x/, '') : null;
}
