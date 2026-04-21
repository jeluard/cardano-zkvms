/**
 * Client-side helpers for the web verifier UI.
 *
 * The browser checks the program output, commitment, and final STARK proof
 * locally.
 */

export function normalizePublicValuesHex(hex) {
  return typeof hex === 'string' ? hex.replace(/^0x/, '') : null;
}
