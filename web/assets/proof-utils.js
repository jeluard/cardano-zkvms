/**
 * Client-side proof processing utilities for OpenVM STARK verification.
 *
 * Handles VK construction (BabyBear/bitcode encoding) and proof byte conversion
 * (hex decode + concatenate + zstd compress). All pure computation — no server needed.
 *
 * ## agg_stark.vk
 *
 * The aggregation STARK verifying key (`agg_stark.vk`) is generated once per
 * OpenVM toolchain version by running:
 *
 *   cargo openvm setup
 *
 * This writes `agg_stark.vk` (along with `agg_stark.pk`, `agg_halo2.pk`, etc.)
 * to `~/.openvm/`. The file is a bitcode-encoded `AggStarkVerifyingKey` that
 * captures the circuit structure of the aggregation STARK — it is deterministic
 * and identical for every user on the same toolchain version.
 *
 * At build time the file is copied from `web/data/agg_stark.vk` into the dist
 * bundle and served as a static asset at `/data/agg_stark.vk`.
 *
 * At runtime the client fetches this static file, then appends two
 * program-specific commits (app_exe_commit + app_vm_commit, bitcode-encoded in
 * BabyBear Montgomery form) to produce the full `VmStarkVerifyingKey` used by
 * the STARK verifier.
 */

const BABYBEAR_P = 2013265921n; // BabyBear modulus: 2^31 - 2^27 + 1
const MONTY_R = 1n << 32n;     // Montgomery radix

/**
 * Convert a hex string (with optional 0x prefix) to Uint8Array.
 */
export function hexToBytes(hex) {
  const clean = hex.replace(/^0x/, '');
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < clean.length; i += 2)
    bytes[i / 2] = parseInt(clean.substring(i, i + 2), 16);
  return bytes;
}

/**
 * Parse 32-byte commit hex → 8 canonical u32 values via base-p decomposition.
 * Follows OpenVM SDK's `bytes_to_u32_digest`.
 */
function commitHexToCanonicalU32s(hexStr) {
  const raw = hexToBytes(hexStr);
  let bigint = 0n;
  for (const byte of raw) bigint = (bigint << 8n) | BigInt(byte);
  const result = [];
  for (let i = 0; i < 8; i++) {
    result.push(Number(bigint % BABYBEAR_P));
    bigint = bigint / BABYBEAR_P;
  }
  return result;
}

/** Convert canonical u32 to Montgomery form: (val * 2^32) mod p */
function toMonty(canonical) {
  return Number((BigInt(canonical) * MONTY_R) % BABYBEAR_P);
}

/**
 * Encode a u32 in bitcode 0.6 serde format (1-byte packing header + data).
 *   val > 65535:  0x00 + 4 bytes LE
 *   val > 255:    0x02 + 2 bytes LE
 *   val <= 255:   0x04 + 1 byte
 */
function bitcodeEncodeU32(val) {
  if (val > 65535) {
    const buf = new Uint8Array(5);
    buf[0] = 0x00;
    buf[1] = val & 0xff;
    buf[2] = (val >> 8) & 0xff;
    buf[3] = (val >> 16) & 0xff;
    buf[4] = (val >> 24) & 0xff;
    return buf;
  } else if (val > 255) {
    const buf = new Uint8Array(3);
    buf[0] = 0x02;
    buf[1] = val & 0xff;
    buf[2] = (val >> 8) & 0xff;
    return buf;
  } else {
    const buf = new Uint8Array(2);
    buf[0] = 0x04;
    buf[1] = val & 0xff;
    return buf;
  }
}

/**
 * Encode commit hex as bitcode-serialized [BabyBear; 8] (Com<SC>).
 * canonical → Montgomery → bitcode u32 encoding
 */
function encodeCommitAsBitcode(hexStr) {
  const canonical = commitHexToCanonicalU32s(hexStr);
  const parts = canonical.map(c => bitcodeEncodeU32(toMonty(c)));
  const totalLen = parts.reduce((s, p) => s + p.length, 0);
  const result = new Uint8Array(totalLen);
  let offset = 0;
  for (const p of parts) {
    result.set(p, offset);
    offset += p.length;
  }
  return result;
}

// ——— VK Construction ———

/**
 * Construct VmStarkVerifyingKey bytes from agg_stark.vk + commit hex strings.
 *
 * VmStarkVerifyingKey = AggVerifyingKey ++ exe_commit_bitcode ++ vm_commit_bitcode
 *
 * @param {Uint8Array} aggVkBytes - Raw agg_stark.vk file contents (bitcode-encoded AggVerifyingKey)
 * @param {string} exeCommitHex - 64-char hex string for app_exe_commit
 * @param {string} vmCommitHex - 64-char hex string for app_vm_commit
 * @returns {Uint8Array} Complete VmStarkVerifyingKey bytes
 */
export function constructVmStarkVk(aggVkBytes, exeCommitHex, vmCommitHex) {
  const exeEncoded = encodeCommitAsBitcode(exeCommitHex);
  const vmEncoded = encodeCommitAsBitcode(vmCommitHex);
  const vk = new Uint8Array(aggVkBytes.length + exeEncoded.length + vmEncoded.length);
  vk.set(aggVkBytes);
  vk.set(exeEncoded, aggVkBytes.length);
  vk.set(vmEncoded, aggVkBytes.length + exeEncoded.length);
  return vk;
}

// ——— Proof Processing ———

/**
 * Convert STARK proof JSON to raw proof bytes (proof || user_public_values).
 *
 * @param {{ proof: string, user_public_values: string }} proofJson - Parsed proof JSON
 * @returns {{ proofBytes: Uint8Array, userPublicValuesHex: string }} Raw proof bytes and upv hex
 */
export function buildProofBytes(proofJson) {
  const proofHex = proofJson.proof;
  const upvHex = proofJson.user_public_values;

  const proofBin = hexToBytes(proofHex);
  const upvBin = hexToBytes(upvHex);

  const combined = new Uint8Array(proofBin.length + upvBin.length);
  combined.set(proofBin, 0);
  combined.set(upvBin, proofBin.length);

  // Strip 0x prefix for display
  const cleanUpvHex = upvHex.replace(/^0x/, '');

  return { proofBytes: combined, userPublicValuesHex: cleanUpvHex };
}

// ——— Zstd Compression ———

let _zstdSimple = null;

/**
 * Initialize the zstd-codec WASM module. Must be called before zstdCompress.
 * Safe to call multiple times (no-op after first init).
 */
export async function initZstd() {
  if (_zstdSimple) return;
  const { ZstdCodec } = await import('zstd-codec');
  return new Promise((resolve, reject) => {
    ZstdCodec.run(zstd => {
      _zstdSimple = new zstd.Simple();
      resolve();
    });
  });
}

/**
 * Compress data using zstd (level 3).
 * initZstd() must have been called first.
 *
 * @param {Uint8Array} data - Data to compress
 * @returns {Uint8Array} Compressed data
 */
export function zstdCompress(data) {
  if (!_zstdSimple) throw new Error('zstd not initialized — call initZstd() first');
  const result = _zstdSimple.compress(data, 3);
  if (!result) throw new Error('zstd compression failed');
  return result;
}

/**
 * Check if zstd is initialized and ready.
 */
export function isZstdReady() {
  return _zstdSimple !== null;
}

/**
 * Process a STARK proof JSON into compressed bytes ready for verify_stark().
 *
 * @param {{ proof: string, user_public_values: string }} proofJson
 * @returns {{ compressedProof: Uint8Array, userPublicValuesHex: string }}
 */
export function processProof(proofJson) {
  const { proofBytes, userPublicValuesHex } = buildProofBytes(proofJson);
  const compressedProof = zstdCompress(proofBytes);
  return { compressedProof, userPublicValuesHex };
}
