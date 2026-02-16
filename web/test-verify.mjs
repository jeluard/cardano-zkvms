#!/usr/bin/env node
/**
 * E2E test script for OpenVM STARK verification.
 *
 * Tests both:
 *   1. Native verification via `cargo openvm verify stark`
 *   2. WASM verification via @ethproofs/openvm-wasm-stark-verifier
 *
 * Usage:
 *   cd web && node test-verify.mjs
 */

import { readFileSync, writeFileSync, unlinkSync } from "fs";
import { execSync } from "child_process";
import { join } from "path";
import { homedir } from "os";

// ─── Paths ───────────────────────────────────────────────────────────────────
const ROOT = new URL("../", import.meta.url).pathname;
const GUEST_DIR = join(ROOT, "crates/zkvms/openvm");
const PROOF_PATH = join(GUEST_DIR, "openvm-guest.stark.proof");
const COMMIT_PATH = join(ROOT, "target/openvm/release/openvm-guest.commit.json");
const AGG_VK_PATH = join(homedir(), ".openvm/agg_stark.vk");
const WASM_PKG = join(
  new URL(".", import.meta.url).pathname,
  "node_modules/@ethproofs/openvm-wasm-stark-verifier/pkg"
);

// ─── Constants ───────────────────────────────────────────────────────────────
const BABYBEAR_P = 2013265921n; // BabyBear modulus: 2^31 - 2^27 + 1
const MONTY_R = 1n << 32n;     // Montgomery radix

// ─── Helpers ─────────────────────────────────────────────────────────────────

function hexToBytes(hex) {
  const clean = hex.replace(/^0x/, "");
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

/**
 * Construct VmStarkVerifyingKey bytes from agg_stark.vk + commit hex strings.
 */
function constructVmStarkVk(aggVk, exeCommitHex, vmCommitHex) {
  const exeEncoded = encodeCommitAsBitcode(exeCommitHex);
  const vmEncoded = encodeCommitAsBitcode(vmCommitHex);
  const vk = new Uint8Array(aggVk.length + exeEncoded.length + vmEncoded.length);
  vk.set(aggVk);
  vk.set(exeEncoded, aggVk.length);
  vk.set(vmEncoded, aggVk.length + exeEncoded.length);
  return vk;
}

/** Build proof bytes from SDK JSON format: hex decode + concatenate + zstd compress */
function buildProofBytes(proofJson) {
  const proofBin = hexToBytes(proofJson.proof);
  const upvBin = hexToBytes(proofJson.user_public_values);
  const combined = new Uint8Array(proofBin.length + upvBin.length);
  combined.set(proofBin, 0);
  combined.set(upvBin, proofBin.length);
  return combined;
}

function zstdCompress(data) {
  const tmpIn = "/tmp/_test_verify_in.bin";
  const tmpOut = "/tmp/_test_verify_out.zst";
  writeFileSync(tmpIn, data);
  execSync(`zstd -f -3 "${tmpIn}" -o "${tmpOut}"`, { stdio: "pipe" });
  const compressed = new Uint8Array(readFileSync(tmpOut));
  try { unlinkSync(tmpIn); unlinkSync(tmpOut); } catch {}
  return compressed;
}

// ─── Load WASM module ────────────────────────────────────────────────────────

async function loadWasm() {
  const wasmBuf = readFileSync(join(WASM_PKG, "openvm_wasm_stark_verifier_bg.wasm"));
  const bg = await import(join(WASM_PKG, "openvm_wasm_stark_verifier_bg.js"));
  const { instance } = await WebAssembly.instantiate(wasmBuf, {
    "./openvm_wasm_stark_verifier_bg.js": bg,
  });
  bg.__wbg_set_wasm(instance.exports);
  if (typeof bg.__wbindgen_init_externref_table === "function")
    bg.__wbindgen_init_externref_table();
  return bg;
}

// ─── Main ────────────────────────────────────────────────────────────────────

async function main() {
  console.log("OpenVM STARK Verification E2E Test\n");

  // 1. Load inputs
  const aggVk = new Uint8Array(readFileSync(AGG_VK_PATH));
  const commitJson = JSON.parse(readFileSync(COMMIT_PATH, "utf8"));
  const proofJson = JSON.parse(readFileSync(PROOF_PATH, "utf8"));

  console.log(`  agg_stark.vk:  ${aggVk.length} bytes`);
  console.log(`  exe commit:    ${commitJson.app_exe_commit}`);
  console.log(`  vm  commit:    ${commitJson.app_vm_commit}`);
  console.log(`  proof version: ${proofJson.version}`);

  // 2. Build VK and proof
  const vk = constructVmStarkVk(aggVk, commitJson.app_exe_commit, commitJson.app_vm_commit);
  const combined = buildProofBytes(proofJson);
  const compressedProof = zstdCompress(combined);

  console.log(`  VK size:       ${vk.length} bytes (agg + ${vk.length - aggVk.length} commit bytes)`);
  console.log(`  proof:         ${combined.length} bytes -> ${compressedProof.length} compressed`);

  // 3. Native verification
  console.log("\n" + "=".repeat(60));
  console.log("  NATIVE: cargo openvm verify stark");
  console.log("=".repeat(60));
  try {
    const out = execSync(
      `cargo openvm verify stark --proof "${PROOF_PATH}" --app-commit "${COMMIT_PATH}"`,
      { cwd: GUEST_DIR, stdio: "pipe", timeout: 120000 }
    );
    console.log("  " + out.toString().trim());
    console.log("  NATIVE: PASS");
  } catch (e) {
    const stderr = e.stderr?.toString().trim() || String(e);
    console.log("  " + stderr.slice(-500));
    console.log("  NATIVE: FAIL");
  }

  // 4. WASM verification
  console.log("\n" + "=".repeat(60));
  console.log("  WASM: @ethproofs/openvm-wasm-stark-verifier");
  console.log("=".repeat(60));

  let wasm;
  try {
    wasm = await loadWasm();
    console.log("  WASM module loaded OK");
  } catch (e) {
    console.error(`  WASM load failed: ${e}\n${e.stack}`);
    process.exit(1);
  }

  try {
    const t0 = performance.now();
    const ok = wasm.verify_stark(compressedProof, vk);
    const dt = ((performance.now() - t0) / 1000).toFixed(1);
    console.log(`  verify_stark returned: ${ok} (${dt}s)`);
    console.log(`  WASM: ${ok ? "PASS" : "FAIL"}`);
  } catch (e) {
    console.log(`  ERROR: ${e}`);
    console.log("  WASM: FAIL");
  }

  // 5. Summary
  console.log("\n" + "=".repeat(60));
  console.log("  DONE");
  console.log("=".repeat(60) + "\n");
}

main().catch(e => { console.error("Fatal:", e); process.exit(1); });
