#!/usr/bin/env node
/**
 * E2E test script for OpenVM 2.0 web verification.
 *
 * Uses the same contract as the browser UI:
 *   1. POST /api/prove to obtain a versioned STARK proof + verification baseline
 *   2. POST /api/verify to run native proof verification on the backend
 *
 * Usage:
 *   cd web && node test-verify.mjs
 */

const BACKEND_URL = (process.env.BACKEND_URL || "http://127.0.0.1:8080").replace(/\/$/, "");
const PROGRAM_HEX = process.env.PROGRAM_HEX || "010000481501";

function backendFetchHint(error) {
  const code = error?.cause?.code;
  if (!code) return null;

  if (code === "ECONNREFUSED") {
    return `No backend is listening at ${BACKEND_URL}. Start the backend first, for example with \`make web-with-backend\` or by running the backend crate directly.`;
  }

  if (code === "ECONNRESET") {
    return `The backend connection to ${BACKEND_URL} was reset. This usually means the backend crashed or is starting with stale OpenVM artifacts. Run \`make build\`, then restart the backend and try again.`;
  }

  return null;
}

async function fetchJson(url, init, label) {
  try {
    const response = await fetch(url, init);
    const json = await response.json();
    return { response, json };
  } catch (error) {
    const hint = backendFetchHint(error);
    if (hint) {
      throw new Error(`${label} failed: ${hint}`);
    }
    throw error;
  }
}

// ─── Main ────────────────────────────────────────────────────────────────────

async function main() {
  console.log("OpenVM 2.0 Web Verification E2E Test\n");

  // 1. Check backend
  const { response: healthResp, json: health } = await fetchJson(
    `${BACKEND_URL}/api/health`,
    undefined,
    "Backend health check",
  );
  if (!healthResp.ok) {
    throw new Error(`Backend health check failed with HTTP ${healthResp.status}`);
  }
  console.log(`  backend:        ${BACKEND_URL}`);
  console.log(`  openvm version: ${health.openvm_version || "unknown"}`);

  // 2. Request proof generation from backend
  const { response: proveResp, json: prove } = await fetchJson(
    `${BACKEND_URL}/api/prove`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ program_hex: PROGRAM_HEX }),
    },
    "Proof generation",
  );
  if (!proveResp.ok || !prove.success) {
    throw new Error(`Proof generation failed: ${prove.error || `HTTP ${proveResp.status}`}`);
  }

  if (!prove.stark_proof_json || !prove.verification_baseline_json) {
    const availableKeys = Object.keys(prove).sort().join(", ") || "(none)";
    throw new Error(
      "Proof generation succeeded but the response is missing verification artifacts. " +
        "Expected stark_proof_json/verification_baseline_json. " +
        `Available keys: ${availableKeys}. Rebuild and restart the backend if it is stale.`,
    );
  }

  // 3. Native verification via backend
  console.log(`  proof version: ${prove.proof_version || prove.stark_proof_json?.version || "unknown"}`);
  console.log(`  commitment:    ${prove.commitment || "n/a"}`);

  // 4. Native backend verification
  console.log("\n" + "=".repeat(60));
  console.log("  Backend: /api/verify");
  console.log("=".repeat(60));

  try {
    const { response: verifyResp, json: verify } = await fetchJson(
      `${BACKEND_URL}/api/verify`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          stark_proof_json: prove.stark_proof_json,
          verification_baseline_json: prove.verification_baseline_json,
        }),
      },
      "Proof verification",
    );

    console.log(`  verify success: ${verify.success}`);
    console.log(`  verified:       ${verify.verified}`);
    console.log(`  duration:       ${(verify.duration_secs ?? 0).toFixed(1)}s`);

    if (!verifyResp.ok || !verify.success || !verify.verified) {
      console.log(`  error:          ${verify.error || `HTTP ${verifyResp.status}`}`);
      console.log("  BACKEND: FAIL");
      process.exit(1);
    }

    console.log("  BACKEND: PASS");
  } catch (e) {
    console.log(`  ERROR: ${e}`);
    console.log("  BACKEND: FAIL");
    process.exit(1);
  }

  // 5. Summary
  console.log("\n" + "=".repeat(60));
  console.log("  DONE");
  console.log("=".repeat(60) + "\n");
}

main().catch(e => { console.error("Fatal:", e); process.exit(1); });
