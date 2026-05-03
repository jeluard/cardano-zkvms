// ——— State ———
let uplcWasm = null;
let aikenWasm = null;
let openVmVerifierWasm = null;
let aggStarkVkBytes = null;
let starkProofJson = null;
let starkVerificationBaselineJson = null;
let starkProofVersion = null;
let mcuHalo2Artifacts = null;
const mcuHalo2ArtifactCache = new Map();
let lastProofDetails = null;
let lastEvalResult = null;
let lastUserPublicValues = null;
let backendAvailable = false;
let selectedMcuDevice = null;
let activeTab = 'aiken';
let aikenCompiled = false;
let compiledHex = null;  // Hex from successful Aiken compilation
let backendStatus = 'unknown';  // 'unknown' | 'available' | 'unavailable'
let proveAbort = null;   // AbortController for in-flight prove request
let proveGeneration = 0; // bumped each run to detect stale callbacks
let busy = false;        // true while an async action is running

// Step completion state (indices 0-4 for steps 1-5)
const stepDone = [false, false, false, false, false];

const MCU_BLE_DEFAULTS = {
  service_uuid: '7b7c0001-78f1-4f9a-8b29-6f1f1d95a100',
  control_uuid: '7b7c0002-78f1-4f9a-8b29-6f1f1d95a100',
  data_uuid: '7b7c0003-78f1-4f9a-8b29-6f1f1d95a100',
  status_uuid: '7b7c0004-78f1-4f9a-8b29-6f1f1d95a100',
  chunk_bytes: 180,
};

const MCU_DEVICE_STORAGE_KEY = 'openvm.mcu.bluetoothDevice';
const MCU_ARTIFACT_STORAGE_KEY = 'openvm.mcu.halo2Artifacts.v2';
const MCU_DEVICE_NAMES = new Set(['ZKMCU', 'OpenVM MCU', 'NimBLE', 'nimble']);
const MCU_ARTIFACT_CACHE_LIMIT = 2;
const MCU_ARTIFACT_CACHE_VERSION = 2;
const MCU_TIMING_PHASES = [
  ['proof', 'Proof'],
  ['connect', 'Connect'],
  ['upload', 'Upload'],
  ['verify', 'Verify'],
];

restoreMcuHalo2ArtifactCache();

// ——— Client-side proof processing ———
import { normalizePublicValuesHex } from './proof-utils.js';
import { highlightAiken } from './aiken-highlight.js';
import { highlightUplc } from './uplc-highlight.js';
import { config } from './config.js';

// ——— WASM Loading ———

const staticAggStarkVkUrl = new URL('../data/agg_stark.vk', import.meta.url);

function getAggStarkVkUrls() {
  return [...new Set([
    config.apiUrl('/data/agg_stark.vk'),
    staticAggStarkVkUrl.href,
  ])];
}

async function loadAggStarkVk() {
  let lastError = null;

  for (const url of getAggStarkVkUrls()) {
    try {
      const resp = await fetch(url);
      if (!resp.ok) {
        throw new Error(`Failed to load agg_stark.vk from ${url} (${resp.status})`);
      }

      return new Uint8Array(await resp.arrayBuffer());
    } catch (error) {
      lastError = error;
    }
  }

  throw lastError ?? new Error('Failed to load agg_stark.vk');
}

async function loadUplcWasm() {
  try {
    const mod = await import('../uplc/uplc_wasm.js');
    await mod.default();
    uplcWasm = mod;
    setStatus('uplcStatus', 'ready', 'UPLC WASM');
    updateSteps();
  } catch (e) {
    setStatus('uplcStatus', 'error', 'UPLC WASM');
  }
}

async function loadAikenWasm() {
  try {
    const mod = await import('../aiken/aiken_wasm.js');
    await mod.default();
    aikenWasm = mod;
    setStatus('aikenStatus', 'ready', 'Aiken WASM');
    document.getElementById('compileBtn').disabled = false;
  } catch (e) {
    setStatus('aikenStatus', 'error', 'Aiken WASM');
  }
}

async function loadOpenVmVerifierWasm() {
  try {
    const mod = await import('../openvm-verifier/openvm_wasm_verifier.js');
    await mod.default();

    aggStarkVkBytes = await loadAggStarkVk();
    openVmVerifierWasm = mod;
    setStatus('starkStatus', 'ready', 'WASM Verify');
    updateSteps();
  } catch (e) {
    openVmVerifierWasm = null;
    aggStarkVkBytes = null;
    setStatus('starkStatus', 'error', 'WASM Verify');
    console.error('Failed to load OpenVM verifier WASM:', e);
    updateSteps();
  }
}

async function checkBackend() {
  try {
    const resp = await fetch(config.apiUrl('/api/health'), { signal: AbortSignal.timeout(3000) });
    if (resp.ok) {
      backendAvailable = true;
      backendStatus = 'available';
      setStatus('backendStatus', 'ready', 'Backend');
      hideBackendBanner();
      updateProofUIVisibility();
      updateSteps();
    } else {
      throw new Error('not ok');
    }
  } catch (e) {
    backendAvailable = false;
    backendStatus = 'unavailable';
    setStatus('backendStatus', 'error', 'Backend');
    showBackendBanner();
    updateProofUIVisibility();
    updateSteps();
  }
}

function setStatus(id, state, label) {
  const el = document.getElementById(id);
  el.className = `status-badge ${state}`;
  const dot = state === 'ready' ? '&#x2713;' : state === 'error' ? '&#x2717;' : '<span class="status-dot"></span>';
  el.innerHTML = `${dot} ${label}`;
}

function isStep1Complete() {
  if (activeTab === 'aiken') return aikenCompiled;
  return document.getElementById('programHex').value.trim() !== '';
}

function updateSteps() {
  const s1ok = isStep1Complete();
  stepDone[0] = s1ok;

  // Step 1 card: always enabled
  setCardState('card1', true, s1ok);

  // Step 2: enabled if step 1 complete + UPLC WASM (backend optional; evaluate works locally)
  const s2ready = s1ok && uplcWasm;
  setCardState('card2', s2ready, stepDone[1]);
  document.getElementById('evalProveBtn').disabled = busy || !s2ready;

  // Step 3: enabled if step 2 done
  setCardState('card3', stepDone[1], stepDone[2]);
  document.getElementById('commitBtn').disabled = busy || !stepDone[1];

  // Step 4: enabled if step 3 is done and the browser verifier is ready.
  const starkVerifierReady = !!openVmVerifierWasm && !!aggStarkVkBytes;
  const s4ready = stepDone[2] && starkVerifierReady && starkProofJson && starkVerificationBaselineJson;
  setCardState('card4', stepDone[2] && starkVerifierReady, stepDone[3]);
  document.getElementById('starkBtn').disabled = busy || !s4ready;

  const hasBluetooth = 'bluetooth' in navigator;
  const mcuBleReady = s1ok && backendAvailable && uplcWasm && hasBluetooth;
  setCardState('card5', hasBluetooth, stepDone[4]);
  document.getElementById('mcuBleBtn').disabled = busy || !mcuBleReady;
  updateMcuForgetButton();

  // Compile button (step 0)
  document.getElementById('compileBtn').disabled = busy || !aikenWasm;

  // Pipeline indicators
  for (let i = 1; i <= 5; i++) {
    const el = document.getElementById(`pipeStep${i}`);
    el.classList.remove('active', 'done', 'fail');
    if (stepDone[i - 1]) el.classList.add('done');
  }
  for (let i = 1; i <= 4; i++) {
    const el = document.getElementById(`pipeConn${i}`);
    el.classList.remove('done', 'fail');
    if (stepDone[i - 1]) el.classList.add('done');
  }

  // Verdict
  if (stepDone[3]) showVerdict(true);
}

function setCardState(id, enabled, done) {
  const el = document.getElementById(id);
  el.classList.toggle('disabled', !enabled);
  el.classList.toggle('done', !!done);
  el.classList.remove('fail');
}

function setCardFail(id) {
  const el = document.getElementById(id);
  el.classList.remove('done');
  el.classList.add('fail');
}

function resetFrom(step) {
  for (let i = step; i < 5; i++) stepDone[i] = false;
  if (step <= 1) {
    // Cancel any in-flight prove request
    if (proveAbort) { proveAbort.abort(); proveAbort = null; }
    proveGeneration++;
    busy = false;
    lastEvalResult = null;
    lastUserPublicValues = null;
    starkProofJson = null;
    starkVerificationBaselineJson = null;
    starkProofVersion = null;
    mcuHalo2Artifacts = null;
    lastProofDetails = null;
    aikenCompiled = false;
    compiledHex = null;
    document.getElementById('toggleUplcPreview').disabled = true;
    // Reset UPLC preview state
    const previewContainer = document.getElementById('uplcPreviewContainer');
    previewContainer.style.display = 'none';
    document.getElementById('toggleUplcPreview').innerHTML = '<span id="toggleUplcPreviewText">▸ Show UPLC</span>';
    document.getElementById('uplcPreview').textContent = 'No program yet. Compile Aiken source or provide UPLC hex to see the human-readable form.';
    hideResult('evalResult');
    hideResult('proveResult');
    document.getElementById('downloadRow').style.display = 'none';
    document.getElementById('proofInfo').textContent = '';
    renderProofDetails();
    document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
  }
  if (step <= 2) hideResult('commitResult');
  if (step <= 3) hideResult('starkResult');
  if (step <= 4) {
    hideResult('mcuBleResult');
    document.getElementById('mcuProofInfo').textContent = '';
  }
  document.getElementById('verdict').className = 'verdict';
  updateSteps();
}

function hideResult(id) {
  document.getElementById(id).className = 'result-box';
}

// Get the current program hex based on active tab
function getCurrentHex() {
  if (activeTab === 'aiken' && compiledHex) {
    return compiledHex;
  }
  return document.getElementById('programHex').value.trim();
}

function switchTab(tab) {
  activeTab = tab;
  document.getElementById('tabBtnAiken').classList.toggle('active', tab === 'aiken');
  document.getElementById('tabBtnUplcHex').classList.toggle('active', tab === 'uplcHex');
  document.getElementById('tabAiken').classList.toggle('active', tab === 'aiken');
  document.getElementById('tabUplcHex').classList.toggle('active', tab === 'uplcHex');
  
  // Handle UPLC preview state based on tab
  const toggle = document.getElementById('toggleUplcPreview');
  const previewContainer = document.getElementById('uplcPreviewContainer');
  
  // Always hide preview and reset button when switching tabs
  previewContainer.style.display = 'none';
  toggle.innerHTML = '<span id="toggleUplcPreviewText">▸ Show UPLC</span>';
  document.getElementById('uplcPreview').textContent = 'No program yet. Compile Aiken source or provide UPLC hex to see the human-readable form.';
  
  if (tab === 'uplcHex') {
    // On hex tab: enable toggle if hex is provided
    const hexValue = document.getElementById('programHex').value.trim();
    toggle.disabled = !hexValue;
  } else if (tab === 'aiken') {
    // On Aiken tab: enable toggle only if Aiken was compiled
    toggle.disabled = !aikenCompiled;
  }
  
  updateSteps();
};

// ——— UPLC Display ———

function updateUplcDisplay() {
  const preview = document.getElementById('uplcPreview');
  
  // Determine which hex to display based on active tab
  const hex = getCurrentHex();
  
  if (!hex) {
    preview.textContent = 'No program yet. Compile Aiken source or provide UPLC hex to see the human-readable form.';
    return;
  }
  
  if (!uplcWasm) {
    preview.textContent = 'UPLC WASM module not yet loaded...';
    return;
  }
  
  try {
    // Convert hex to human-readable UPLC
    const readable = uplcWasm.hex_to_uplc(hex);
    // Apply syntax highlighting
    const highlighted = highlightUplc(readable);
    preview.innerHTML = highlighted;
  } catch (e) {
    preview.textContent = `Error converting hex to UPLC: ${escapeHtml(String(e))}`;
  }
}

function toggleUplcPreview() {
  const container = document.getElementById('uplcPreviewContainer');
  const btn = document.getElementById('toggleUplcPreview');
  const isVisible = container.style.display !== 'none';
  
  if (isVisible) {
    container.style.display = 'none';
    btn.innerHTML = '<span id="toggleUplcPreviewText">▸ Show UPLC</span>';
  } else {
    container.style.display = 'block';
    btn.innerHTML = '<span id="toggleUplcPreviewText">▾ Hide UPLC</span>';
    updateUplcDisplay();  // Render preview when showing
  }
}

// ——— Aiken Compilation ———

async function compileAiken() {
  if (!aikenWasm) return;
  const source = document.getElementById('aikenSource').value;
  if (!source.trim()) return showResult('compileResult', 'error', 'Please enter Aiken source code.');

  resetFrom(1);

  const btn = document.getElementById('compileBtn');
  btn.disabled = true;
  document.getElementById('compileBtnText').innerHTML = '<span class="spinner"></span> Compiling…';

  await new Promise(resolve => setTimeout(resolve, 0));

  try {
    const t0 = performance.now();
    const hex = aikenWasm.compile_to_uplc_hex(source);
    const dt = performance.now() - t0;
    compiledHex = hex;  // Store the compiled hex (don't overwrite programHex input)
    aikenCompiled = true;
    updateUplcDisplay();  // Update the UPLC preview with compiled hex
    document.getElementById('toggleUplcPreview').disabled = false;  // Enable the Show UPLC button
    updateSteps();
    showResult('compileResult', 'success',
      `<div class="result-label">Compiled to UPLC</div>` +
      `<div class="result-value">${hex.length > 120 ? hex.slice(0, 120) + '…' : hex}</div>` +
      `<div class="timing">Compiled in ${dt.toFixed(1)} ms &mdash; ${hex.length / 2} bytes flat</div>`
    );
  } catch (e) {
    aikenCompiled = false;
    compiledHex = null;
    updateSteps();
    showResult('compileResult', 'error',
      `<div class="result-label">Compilation Failed</div>` +
      `<div class="result-value">${escapeHtml(String(e))}</div>`
    );
  }
  btn.disabled = false;
  document.getElementById('compileBtnText').textContent = 'Compile';
};

// ——— Step 2: Evaluation & Proof Generation ———

async function runEvaluateAndProve() {
  const hex = getCurrentHex();
  if (!hex) return;

  // Clear previous evaluation/proof results
  // On Aiken tab with compilation, preserve the compiled state
  // On other cases, clear everything including previous compilations
  if (activeTab === 'aiken' && aikenCompiled) {
    resetFrom(2);
  } else {
    resetFrom(1);
  }
  
  // Re-enable toggle on hex tab if hex input is available
  if (activeTab === 'uplcHex') {
    const hexValue = document.getElementById('programHex').value.trim();
    if (hexValue) {
      document.getElementById('toggleUplcPreview').disabled = false;
    }
  }
  const myGeneration = ++proveGeneration;
  const abort = new AbortController();
  proveAbort = abort;

  busy = true;
  updateSteps();
  document.getElementById('evalProveBtnText').innerHTML = '<span class="spinner"></span> Evaluating & Proving…';
  setPipeActive(2);

  // 1. Local WASM evaluation
  let evalResult;
  try {
    const t0 = performance.now();
    evalResult = uplcWasm.evaluate_uplc(hex);
    const dt = performance.now() - t0;
    lastEvalResult = evalResult;
    showResult('evalResult', 'success',
      `<div class="result-label">Evaluation Result</div>` +
      `<div class="result-value">${escapeHtml(evalResult)}</div>` +
      `<div class="timing">Evaluated in ${dt.toFixed(1)} ms</div>`
    );
    document.getElementById('expectedResult').value = evalResult;
  } catch (e) {
    setPipeFail(2);
    setCardFail('card2');
    showResult('evalResult', 'error',
      `<div class="result-label">Evaluation Failed</div>` +
      `<div class="result-value">${escapeHtml(String(e))}</div>`
    );
    busy = false;
    updateSteps();
    document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
    return;
  }

  // 2. Request proof generation from backend (if available)
  if (!backendAvailable) {
    showResult('proveResult', 'info',
      `<div class="result-label">Proof Generation</div>` +
      `<div class="result-value">Backend unavailable. Evaluation complete, but proof generation skipped.</div>`
    );
    stepDone[1] = true;
    busy = false;
    updateSteps();
    document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
    return;
  }

  showResult('proveResult', 'info',
    `<div class="result-label">Proof Generation</div>` +
    `<div class="result-value">Generating STARK proof on server… this may take a while.</div>`
  );

  try {
    const resp = await fetch(config.apiUrl('/api/prove'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ program_hex: hex }),
      signal: abort.signal,
    });
    if (myGeneration !== proveGeneration) return; // stale
    const data = await resp.json();

    if (!data.success) {
      setPipeFail(2);
      setCardFail('card2');
      showResult('proveResult', 'error',
        `<div class="result-label">Proof Generation Failed</div>` +
        `<div class="result-value">${escapeHtml(data.error || 'Unknown error')}</div>`
      );
      busy = false;
      updateSteps();
      document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
      return;
    }

    const duration = data.duration_secs ? data.duration_secs.toFixed(1) : '?';

    // Check we have the raw proof JSON and verification baseline from backend
    if (!data.stark_proof_json || !data.verification_baseline_json) {
      const availableKeys = Object.keys(data).sort().join(', ') || 'none';
      showResult('proveResult', 'warning',
        `<div class="result-label">Warning</div>` +
        `<div class="result-value">Backend proof response is missing verification artifacts. Rebuild and restart the backend if it is stale.</div>` +
        `<div class="timing">Expected stark_proof_json + verification_baseline_json. Available keys: ${escapeHtml(availableKeys)}</div>`
      );
      stepDone[1] = true;
      busy = false;
      updateSteps();
      document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
      return;
    }

    starkProofJson = data.stark_proof_json;
    starkVerificationBaselineJson = data.verification_baseline_json;
    starkProofVersion = data.proof_version || data.stark_proof_json.version || null;
    lastUserPublicValues = normalizePublicValuesHex(data.commitment);

    const proofJsonSize = fmtBytes(new TextEncoder().encode(JSON.stringify(starkProofJson)).length);
    const baselineJsonSize = fmtBytes(new TextEncoder().encode(JSON.stringify(starkVerificationBaselineJson)).length);
    lastProofDetails = await buildProofDetails(data, proofJsonSize, baselineJsonSize);
    renderProofDetails();
    const verificationStatus = openVmVerifierWasm && aggStarkVkBytes
      ? 'browser WASM ready'
      : 'browser WASM unavailable';
    document.getElementById('proofInfo').textContent =
      `Proof JSON: ${proofJsonSize}. Baseline: ${baselineJsonSize}. Verification: ${verificationStatus}.`;

    showResult('proveResult', 'success',
      `<div class="result-label">Proof Generated</div>` +
      `<div class="result-value">STARK proof generated in ${duration}s via OpenVM ${escapeHtml(data.openvm_version || 'unknown')}.</div>` +
      `<div class="timing">Proof version: ${escapeHtml(starkProofVersion || 'unknown')}, Proof JSON: ${proofJsonSize}</div>`
    );

    // Fill commitment
    if (data.commitment) {
      document.getElementById('expectedCommitment').value = data.commitment;
    }

    // Download buttons
    if (data.commitment) {
      const blob = new Blob([data.commitment], { type: 'text/plain' });
      document.getElementById('dlCommitment').href = URL.createObjectURL(blob);
    }
    if (starkProofJson) {
      const blob = new Blob([JSON.stringify(starkProofJson, null, 2)], { type: 'application/json' });
      document.getElementById('dlProof').href = URL.createObjectURL(blob);
    }
    if (data.commitment || starkProofJson) {
      document.getElementById('downloadRow').style.display = 'flex';
    }

    // Mark step 2 done
    stepDone[1] = true;
    updateSteps();

  } catch (e) {
    if (e.name === 'AbortError' || myGeneration !== proveGeneration) return; // cancelled
    setPipeFail(2);
    setCardFail('card2');
    showResult('proveResult', 'error',
      `<div class="result-label">Request Failed</div>` +
      `<div class="result-value">${escapeHtml(String(e))}</div>`
    );
  }

  if (myGeneration !== proveGeneration) return; // stale
  busy = false;
  updateSteps();
  document.getElementById('evalProveBtnText').innerHTML = getEvalButtonText();
};

// ——— Step 3: Commitment Verification ———

function runCommitmentCheck() {
  if (!uplcWasm || lastEvalResult === null) {
    return showResult('commitResult', 'error', 'Run Evaluate & Prove first.');
  }

  resetFrom(2);

  const hex = getCurrentHex();
  const expectedResult = document.getElementById('expectedResult').value.trim();
  const expectedCommitment = document.getElementById('expectedCommitment').value.trim().toLowerCase();

  setPipeActive(3);

  if (expectedResult && expectedResult !== lastEvalResult) {
    setPipeFail(3);
    stepDone[2] = false;
    setCardFail('card3');
    updateSteps();
    showResult('commitResult', 'error',
      `<div class="result-label">Result Mismatch</div>` +
      `<div>Expected result: ${escapeHtml(expectedResult)}</div>` +
      `<div>Actual result: &nbsp;${escapeHtml(lastEvalResult)}</div>`
    );
    return;
  }

  try {
    const resultForCommitment = expectedResult || lastEvalResult;
    const t0 = performance.now();
    const computed = uplcWasm.compute_commitment(hex, resultForCommitment);
    const dt = performance.now() - t0;

    let details = '';
    if (expectedResult) {
      details += `<div style="margin-bottom:6px">&#x2705; Result matches: <strong>${escapeHtml(expectedResult)}</strong></div>`;
    }

    if (!expectedCommitment) {
      stepDone[2] = true;
      updateSteps();
      showResult('commitResult', 'info',
        details +
        `<div class="result-label">Computed Commitment (SHA-256)</div>` +
        `<div class="result-value">${computed}</div>` +
        `<div class="timing">Computed in ${dt.toFixed(1)} ms</div>`
      );
    } else if (computed === expectedCommitment) {
      stepDone[2] = true;
      updateSteps();
      showResult('commitResult', 'success',
        details +
        `<div class="result-label">Commitment Match</div>` +
        `<div class="result-value">${computed}</div>` +
        `<div class="timing">Verified in ${dt.toFixed(1)} ms</div>`
      );
    } else {
      stepDone[2] = false;
      setPipeFail(3);
      setCardFail('card3');
      updateSteps();
      showResult('commitResult', 'error',
        details +
        `<div class="result-label">Commitment Mismatch</div>` +
        `<div>Expected: ${escapeHtml(expectedCommitment)}</div>` +
        `<div>Computed: ${computed}</div>`
      );
    }
  } catch (e) {
    stepDone[2] = false;
    setPipeFail(3);
    setCardFail('card3');
    updateSteps();
    showResult('commitResult', 'error', escapeHtml(String(e)));
  }
};

// ——— Step 4: STARK Verification ———

async function runStarkVerification() {
  if (!openVmVerifierWasm) {
    return showResult('starkResult', 'error', 'Verifier WASM unavailable. Reload the page or rebuild the verifier bundle.');
  }
  if (!aggStarkVkBytes) {
    return showResult('starkResult', 'error', 'Aggregation verification key unavailable.');
  }
  if (!starkProofJson) {
    return showResult('starkResult', 'error', 'Proof not available. Run Evaluate & Prove first.');
  }
  if (!starkVerificationBaselineJson) {
    return showResult('starkResult', 'error', 'Verification artifacts not available. Run Evaluate & Prove again.');
  }

  resetFrom(3);

  busy = true;
  updateSteps();
  setPipeActive(4);
  showResult('starkResult', 'info',
    `<div class="result-label">STARK Verification</div>` +
    `<div class="result-value">Verifying proof locally in your browser via WASM… this may take a moment.</div>`
  );

  // Yield to allow UI to update
  await new Promise(resolve => setTimeout(resolve, 50));

  try {
    const t0 = performance.now();
    const verified = openVmVerifierWasm.verify_stark(
      JSON.stringify(starkProofJson),
      aggStarkVkBytes,
      JSON.stringify(starkVerificationBaselineJson),
    );
    const dt = (performance.now() - t0) / 1000;

    if (verified) {
      stepDone[3] = true;
      if (lastProofDetails) {
        lastProofDetails.verifier = 'accepted';
        lastProofDetails.verifiedIn = `${dt.toFixed(1)}s`;
        renderProofDetails();
      }
      updateSteps();

      let pvHtml = '';
      if (lastUserPublicValues) {
        pvHtml = `<div class="public-values">` +
          `<div class="pv-label">Proof public values &mdash; SHA-256( program || result )</div>` +
          `<div class="pv-value">${lastUserPublicValues}</div>` +
          `</div>`;
      }

      showResult('starkResult', 'success',
        `<div class="result-label">STARK Proof Verified</div>` +
        `<div class="result-value">The browser's OpenVM verifier confirmed the proof is valid.</div>` +
        `<div class="timing">Verified locally in ${dt.toFixed(1)}s</div>` +
        pvHtml
      );
    } else {
      if (lastProofDetails) {
        lastProofDetails.verifier = 'rejected';
        lastProofDetails.verifiedIn = `${dt.toFixed(1)}s`;
        renderProofDetails();
      }
      setPipeFail(4);
      setCardFail('card4');
      showResult('starkResult', 'error',
        `<div class="result-label">STARK Verification Failed</div>` +
        `<div class="result-value">The browser verifier rejected the proof.</div>` +
        `<div class="timing">Checked locally in ${dt.toFixed(1)}s</div>`
      );
    }
  } catch (e) {
    if (lastProofDetails) {
      lastProofDetails.verifier = 'error';
      renderProofDetails();
    }
    setPipeFail(4);
    setCardFail('card4');
    showResult('starkResult', 'error',
      `<div class="result-label">STARK Verification Error</div>` +
      `<div class="result-value">${escapeHtml(String(e))}</div>`
    );
  }

  busy = false;
  updateSteps();
};

// ——— Step 5: ZKMCU BLE Verification ———

async function runMcuBleVerification() {
  const hex = getCurrentHex();
  if (!hex) return showResult('mcuBleResult', 'error', 'Compile Aiken or provide UPLC hex first.');
  if (!backendAvailable) return showResult('mcuBleResult', 'error', 'Backend unavailable. MCU proof generation needs the server.');
  if (!('bluetooth' in navigator)) return showResult('mcuBleResult', 'error', webBluetoothUnavailableMessage());

  resetFrom(4);
  busy = true;
  updateSteps();
  setPipeActive(5);

  try {
    const timing = emptyMcuTiming();
    const cachedArtifacts = getCachedMcuHalo2Artifacts(hex);

    if (!cachedArtifacts) {
      const proofStartedAt = performance.now();
      const { data } = await getMcuHalo2Artifacts(hex);
      timing.proof = performance.now() - proofStartedAt;
      data._mcuTiming = { proof: timing.proof };
      storeMcuHalo2Artifacts(hex, data);
      populateMcuPvEditor(data.public_values_hex || '');
      const verifierKeyBytes = base64ToBytes(data.verifier_key_b64);
      const proofEnvelopeBytes = base64ToBytes(data.proof_envelope_b64);
      const totalBytes = verifierKeyBytes.length + proofEnvelopeBytes.length;

      document.getElementById('mcuProofInfo').textContent =
        `Proof ${shortHex(data.proof_sha256)} · ${fmtBytes(totalBytes)} BLE payload · ${data.public_values_len || 0} public bytes · cached`;
      showResult('mcuBleResult', 'success',
        `<div class="result-label">MCU Proof Cached</div>` +
        `<div class="result-value">OpenVM Halo2/KZG artifacts are ready. Click Send Cached Proof to choose a ZKMCU device and transfer immediately.</div>` +
        renderMcuTimingBar(timing, 'proof')
      );
      busy = false;
      document.getElementById('mcuBleBtnText').innerHTML = 'Send Cached Proof';
      updateSteps();
      return;
    }

    const data = cachedArtifacts;
    Object.assign(timing, data._mcuTiming || {});
    const verifierKeyBytes = base64ToBytes(data.verifier_key_b64);
    // Read public values from the editor (may have been modified by user)
    const pvEdited = getMcuPvEdited();
    let proofEnvelopeBytes = base64ToBytes(data.proof_envelope_b64);
    let pvTampered = false;
    if (pvEdited !== null && pvEdited !== (data.public_values_hex || '')) {
      const patchResp = await fetch(config.apiUrl('/api/patch-envelope'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ proof_envelope_b64: data.proof_envelope_b64, public_values_hex: pvEdited }),
      });
      const patchData = await patchResp.json();
      if (!patchData.success) throw new Error(`patch-envelope: ${patchData.error}`);
      proofEnvelopeBytes = base64ToBytes(patchData.proof_envelope_b64);
      pvTampered = true;
    }
    const ble = { ...MCU_BLE_DEFAULTS, ...(data.ble || {}) };
    const totalBytes = verifierKeyBytes.length + proofEnvelopeBytes.length;

    document.getElementById('mcuProofInfo').textContent =
      `Proof ${shortHex(data.proof_sha256)} · ${fmtBytes(totalBytes)} BLE payload · ${data.public_values_len || 0} public bytes · cached` +
      (pvTampered ? ' · ⚠ MODIFIED PUBLIC VALUES' : '');

    document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Finding MCU…';
    showResult('mcuBleResult', 'info',
      `<div class="result-label">Connect ZKMCU</div>` +
      `<div class="result-value">Looking for a previously granted MCU device. The Bluetooth picker opens only if this browser has no saved grant.</div>` +
      renderMcuTimingBar(timing, 'connect')
    );

    const device = await getMcuDeviceForTransfer(timing);
    rememberMcuDevice(device);

    showResult('mcuBleResult', 'info',
      `<div class="result-label">ZKMCU Selected</div>` +
      `<div class="result-value">Reusing cached MCU proof artifacts. Connecting to the selected device.</div>` +
      renderMcuTimingBar(timing, 'connect')
    );
    document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Waiting for BLE…';

    const connectStartedAt = performance.now();
    const server = await device.gatt.connect();
    const service = await server.getPrimaryService(ble.service_uuid);
    const controlChar = await service.getCharacteristic(ble.control_uuid);
    const dataChar = await service.getCharacteristic(ble.data_uuid);
    const statusChar = await service.getCharacteristic(ble.status_uuid);

    const statusUpdates = [];
    let browserSentBytes = 0;
    let latestStatusMessage = '';
    let latestReceiveProgress = null;
    const renderTransferProgress = () => renderMcuReceiveProgress({
      totalBytes,
      sentBytes: browserSentBytes,
      receiveProgress: latestReceiveProgress,
    });
    const handleStatusMessage = message => {
      if (!message) return '';
      latestStatusMessage = message;
      const receiveProgress = parseMcuReceiveProgress(message);
      if (receiveProgress) {
        latestReceiveProgress = receiveProgress;
      }
      if (statusUpdates[statusUpdates.length - 1] !== message) {
        statusUpdates.push(message);
      }
      // Step progress messages (e.g. "verifying 4 commitments") are handled by
      // the rAF animation loop; don't overwrite the whole result box.
      const stepInfo = parseMcuVerifyStep(message);
      if (stepInfo) {
        currentVerifyStep = stepInfo;
        return message;
      }
      showResult('mcuBleResult', statusType(message),
        `<div class="result-label">ZKMCU</div>` +
        `<div class="result-value">${escapeHtml(message)}</div>` +
        renderTransferProgress() +
        `<div class="timing">${fmtBytes(totalBytes)} total BLE payload</div>` +
        renderMcuTimingBar(timing, message.includes('verifying') ? 'verify' : 'upload')
      );
      return message;
    };
    const onStatus = event => {
      handleStatusMessage(decodeBleText(event.target.value));
    };
    await statusChar.startNotifications();
    statusChar.addEventListener('characteristicvaluechanged', onStatus);
    timing.connect = performance.now() - connectStartedAt;

    document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Sending artifacts…';
    showResult('mcuBleResult', 'info',
      `<div class="result-label">ZKMCU Upload</div>` +
      `<div class="result-value">Sending ${fmtBytes(totalBytes)} of verifier key and proof envelope.</div>` +
      renderTransferProgress() +
      renderMcuTimingBar(timing, 'upload')
    );
    const uploadStartedAt = performance.now();
    const updateBrowserUploadProgress = delta => {
      browserSentBytes += delta;
      const message = latestStatusMessage.startsWith('receiving') || latestStatusMessage.startsWith('received')
        ? latestStatusMessage
        : `Sending ${fmtBytes(totalBytes)} of verifier key and proof envelope.`;
      showResult('mcuBleResult', 'info',
        `<div class="result-label">ZKMCU Upload</div>` +
        `<div class="result-value">${escapeHtml(message)}</div>` +
        renderTransferProgress() +
        `<div class="timing">${fmtBytes(browserSentBytes)} queued to BLE so far</div>` +
        renderMcuTimingBar(timing, 'upload')
      );
    };
    await writeUtf8(
      controlChar,
      `START ${verifierKeyBytes.length} ${proofEnvelopeBytes.length} ${data.proof_sha256 || ''}`,
      { requireResponse: true }
    );
    const startStatus = await waitForMcuStatus(
      statusChar,
      statusUpdates,
      message => message.startsWith('receiving') || message.startsWith('error'),
      handleStatusMessage,
      'Timed out waiting for ZKMCU to enter receiving state',
      50
    );
    if (startStatus.startsWith('error')) {
      throw new Error(startStatus || 'ZKMCU failed to start receiving proof artifacts');
    }
    // Send key then proof consecutively. The MCU only notifies once when the *full*
    // transfer is complete ("received"), so there is no intermediate per-key progress
    // signal to wait for. Write-with-response provides per-packet flow control.
    await sendBleBlob(dataChar, 1, verifierKeyBytes, ble.chunk_bytes, {
      requireResponse: true,
      onProgress: updateBrowserUploadProgress,
    });
    await sendBleBlob(dataChar, 2, proofEnvelopeBytes, ble.chunk_bytes, {
      requireResponse: true,
      onProgress: updateBrowserUploadProgress,
    });
    const uploadStatus = await waitForMcuStatus(
      statusChar,
      statusUpdates,
      message => message.startsWith('received') || message.startsWith('error'),
      handleStatusMessage,
      'Timed out waiting for ZKMCU to finish receiving proof artifacts',
      250
    );
    timing.upload = performance.now() - uploadStartedAt;
    if (!uploadStatus.startsWith('received')) {
      throw new Error(uploadStatus || 'ZKMCU upload failed before verification started');
    }

    document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Verifying on MCU…';
    showResult('mcuBleResult', 'info',
      `<div class="result-label">ZKMCU Verify</div>` +
      `<div class="result-value">MCU received all proof artifacts. Starting native Halo2/KZG verification.</div>` +
      renderTransferProgress() +
      renderMcuTimingBar(timing, 'verify')
    );
    const verifyStartedAt = performance.now();
    // Estimated duration from prior run; fall back to 65 s
    const verifyEstimateMs = timing.verify || 65000;
    let verifyAnimCancelled = false;
    let currentVerifyStep = null; // { step, label } from MCU progress notifications
    const animateVerifyProgress = () => {
      if (verifyAnimCancelled) return;
      const elapsedMs = performance.now() - verifyStartedAt;
      showResult('mcuBleResult', 'info',
        `<div class="result-label">ZKMCU Verify</div>` +
        `<div class="result-value">Verifying on MCU (Halo2/KZG)\u2026</div>` +
        renderMcuVerifySteps(currentVerifyStep) +
        renderMcuVerifyProgress(elapsedMs, verifyEstimateMs) +
        renderMcuTimingBar(timing, 'verify')
      );
      requestAnimationFrame(animateVerifyProgress);
    };
    requestAnimationFrame(animateVerifyProgress);
    await writeUtf8(controlChar, 'COMMIT', { requireResponse: true });
    const verdict = await waitForMcuVerdict(statusChar, statusUpdates, handleStatusMessage);
    verifyAnimCancelled = true;
    timing.verify = performance.now() - verifyStartedAt;
    // Persist updated verify timing into the artifact cache so future runs have a better estimate
    if (data._mcuTiming) {
      data._mcuTiming.verify = timing.verify;
      storeMcuHalo2Artifacts(hex, data);
    }

    if (verdict.startsWith('verified')) {
      stepDone[4] = true;
      updateSteps();
      showResult('mcuBleResult', 'success',
        `<div class="result-label">MCU Verified</div>` +
        `<div class="result-value">${escapeHtml(verdict)}</div>` +
        renderTransferProgress() +
        `<div class="timing">Proof ${escapeHtml(shortHex(data.proof_sha256))} accepted by the ZKMCU Halo2/KZG verifier.</div>` +
        renderMcuTimingBar(timing)
      );
    } else {
      setPipeFail(5);
      setCardFail('card5');
      showResult('mcuBleResult', 'error',
        `<div class="result-label">MCU Rejected</div>` +
        `<div class="result-value">${escapeHtml(verdict)}</div>` +
        renderTransferProgress() +
        renderMcuTimingBar(timing)
      );
    }
  } catch (error) {
    setPipeFail(5);
    setCardFail('card5');
    showResult('mcuBleResult', 'error',
      `<div class="result-label">MCU BLE Error</div>` +
      `<div class="result-value">${escapeHtml(formatMcuBleError(error))}</div>`
    );
  }

  busy = false;
  document.getElementById('mcuBleBtnText').innerHTML = 'Generate MCU Proof &amp; Send';
  updateSteps();
}

function emptyMcuTiming() {
  return { proof: 0, connect: 0, upload: 0, verify: 0 };
}

function webBluetoothUnavailableMessage() {
  return 'Web Bluetooth is not available here. Open http://localhost:3000 in external Chrome or Edge; VS Code\'s internal browser cannot complete BLE pairing.';
}

function isInternalBrowser() {
  const userAgent = navigator.userAgent || '';
  return /Electron|VSCode|Code - Insiders/i.test(userAgent) || window.location.protocol === 'vscode-webview:';
}

function formatMcuBleError(error) {
  const message = String(error);
  if (isInternalBrowser()) {
    return `${message}. Open http://localhost:3000 in external Chrome or Edge; the VS Code internal browser cannot complete the native Bluetooth device selection.`;
  }
  if (message.includes('NotFoundError')) {
    return `${message}. No device was selected. Choose the Tufty advertising as ZKMCU in the browser Bluetooth picker.`;
  }
  return message;
}

function formatMs(ms) {
  if (!ms) return '-';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(ms < 10000 ? 1 : 0)} s`;
}

function renderMcuTimingBar(timing, activePhase = '') {
  const total = MCU_TIMING_PHASES.reduce((sum, [key]) => sum + Math.max(0, timing[key] || 0), 0);
  const segments = MCU_TIMING_PHASES.map(([key, label]) => {
    const ms = Math.max(0, timing[key] || 0);
    const active = key === activePhase ? ' active' : '';
    const pending = !ms && key !== activePhase ? ' pending' : '';
    const MIN_VISIBLE = 4; // % below which a measured segment is flagged as not-to-scale
    let width;
    let notToScale = false;
    if (total > 0 && ms > 0) {
      const proportional = (ms / total) * 100;
      if (proportional < MIN_VISIBLE) {
        width = MIN_VISIBLE;
        notToScale = true;
      } else {
        width = proportional;
      }
    } else if (key === activePhase) {
      width = 4;
    } else {
      width = 0.5;
    }
    const nts = notToScale ? ' not-to-scale' : '';
    const title = notToScale ? ` title="${label}: ${formatMs(ms)} (not to scale)"` : '';
    return `<div class="mcu-timing-segment phase-${key}${active}${pending}${nts}"${title} style="flex-basis:${width.toFixed(2)}%">` +
      `<span>${label}</span><strong>${ms ? formatMs(ms) : ''}</strong>` +
      `</div>`;
  }).join('');
  const totalText = total ? `Total ${formatMs(total)}` : 'Timing starts when proof artifacts are prepared';

  return `<div class="mcu-timing" aria-label="MCU timing breakdown">` +
    `<div class="mcu-timing-bar">${segments}</div>` +
    `<div class="mcu-timing-total">${totalText}</div>` +
    `</div>`;
}

async function getMcuHalo2Artifacts(hex) {
  const key = mcuArtifactCacheKey(hex);
  const cached = mcuHalo2ArtifactCache.get(key);
  if (cached) {
    touchMcuHalo2ArtifactCache(key, cached);
    document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Reusing proof…';
    showResult('mcuBleResult', 'info',
      `<div class="result-label">MCU Proof</div>` +
      `<div class="result-value">Reusing cached OpenVM Halo2/KZG artifacts for this unchanged UPLC program.</div>`
    );
    return { data: cached, cached: true };
  }

  document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Preparing MCU proof…';
  showResult('mcuBleResult', 'info',
    `<div class="result-label">MCU Proof</div>` +
    `<div class="result-value">Generating OpenVM Halo2/KZG artifacts for ZKMCU. This can take a while.</div>`
  );

  const proofResponse = await fetch(config.apiUrl('/api/prove/mcu-halo2'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ program_hex: hex }),
  });
  const data = await proofResponse.json();
  if (!data.success) throw new Error(data.error || 'MCU proof generation failed');

  touchMcuHalo2ArtifactCache(key, data);
  return { data, cached: false };
}

function getCachedMcuHalo2Artifacts(hex) {
  const key = mcuArtifactCacheKey(hex);
  const cached = mcuHalo2ArtifactCache.get(key);
  if (!cached) return null;
  touchMcuHalo2ArtifactCache(key, cached);
  return cached;
}

function storeMcuHalo2Artifacts(hex, data) {
  touchMcuHalo2ArtifactCache(mcuArtifactCacheKey(hex), data);
}

function touchMcuHalo2ArtifactCache(key, data) {
  mcuHalo2ArtifactCache.delete(key);
  mcuHalo2ArtifactCache.set(key, data);
  while (mcuHalo2ArtifactCache.size > MCU_ARTIFACT_CACHE_LIMIT) {
    mcuHalo2ArtifactCache.delete(mcuHalo2ArtifactCache.keys().next().value);
  }
  mcuHalo2Artifacts = data;
  persistMcuHalo2ArtifactCache();
}

function restoreMcuHalo2ArtifactCache() {
  try {
    const stored = JSON.parse(localStorage.getItem(MCU_ARTIFACT_STORAGE_KEY) || 'null');
    if (stored?.version !== MCU_ARTIFACT_CACHE_VERSION || !Array.isArray(stored.entries)) return;
    for (const entry of stored.entries) {
      if (typeof entry?.key === 'string' && entry.data && typeof entry.data === 'object') {
        mcuHalo2ArtifactCache.set(entry.key, entry.data);
      }
    }
    while (mcuHalo2ArtifactCache.size > MCU_ARTIFACT_CACHE_LIMIT) {
      mcuHalo2ArtifactCache.delete(mcuHalo2ArtifactCache.keys().next().value);
    }
  } catch (_) {
    localStorage.removeItem(MCU_ARTIFACT_STORAGE_KEY);
  }
}

function persistMcuHalo2ArtifactCache() {
  try {
    localStorage.setItem(MCU_ARTIFACT_STORAGE_KEY, JSON.stringify({
      version: MCU_ARTIFACT_CACHE_VERSION,
      entries: [...mcuHalo2ArtifactCache.entries()].map(([key, data]) => ({ key, data })),
    }));
  } catch (_) {}
}

function mcuArtifactCacheKey(hex) {
  const normalizedHex = hex.trim().toLowerCase();
  return `${config.backendUrl}|${normalizedHex}`;
}

function rememberMcuDevice(device) {
  selectedMcuDevice = device;
  try {
    localStorage.setItem(MCU_DEVICE_STORAGE_KEY, JSON.stringify({
      id: device.id || '',
      name: device.name || 'MCU',
    }));
  } catch (_) {}
  updateMcuForgetButton();
}

async function getMcuDeviceForTransfer(timing) {
  const device = await findReusableMcuDevice();
  if (device) return device;

  document.getElementById('mcuBleBtnText').innerHTML = '<span class="spinner"></span> Choose MCU…';
  showResult('mcuBleResult', 'info',
    `<div class="result-label">Choose ZKMCU</div>` +
    `<div class="result-value">Select ZKMCU, OpenVM MCU, or NimBLE once. Chrome can reuse the grant on future page loads.</div>` +
    renderMcuTimingBar(timing, 'connect')
  );

  return navigator.bluetooth.requestDevice({
    filters: [
      { name: 'ZKMCU' },
      { namePrefix: 'ZK' },
      { name: 'OpenVM MCU' },
      { namePrefix: 'OpenVM' },
      { name: 'nimble' },
      { name: 'NimBLE' },
      { services: [MCU_BLE_DEFAULTS.service_uuid] },
    ],
    optionalServices: [MCU_BLE_DEFAULTS.service_uuid],
  });
}

async function findReusableMcuDevice() {
  if (selectedMcuDevice) return selectedMcuDevice;
  if (!navigator.bluetooth?.getDevices) return null;

  const remembered = rememberedMcuDevice();
  const grantedDevices = await navigator.bluetooth.getDevices();
  return grantedDevices.find(device => remembered?.id && device.id === remembered.id)
    || grantedDevices.find(isLikelyMcuDevice)
    || null;
}

function rememberedMcuDevice() {
  try {
    return JSON.parse(localStorage.getItem(MCU_DEVICE_STORAGE_KEY) || 'null');
  } catch (_) {
    return null;
  }
}

function isLikelyMcuDevice(device) {
  const name = device?.name || '';
  return MCU_DEVICE_NAMES.has(name) || name.startsWith('ZK') || name.startsWith('OpenVM');
}

function mcuDeviceLabel(device, fallback) {
  return device?.name || fallback?.name || 'MCU';
}

function updateMcuForgetButton() {
  const button = document.getElementById('mcuForgetBtn');
  const label = document.getElementById('mcuForgetBtnText');
  if (!button || !label) return;

  const hasBluetooth = 'bluetooth' in navigator;
  const remembered = rememberedMcuDevice();
  const canLookUpDevices = !!navigator.bluetooth?.getDevices;
  const hasTargetHint = !!selectedMcuDevice || !!remembered || canLookUpDevices;

  button.disabled = busy || !hasBluetooth || !hasTargetHint;
  label.textContent = selectedMcuDevice || remembered
    ? `Forget ${mcuDeviceLabel(selectedMcuDevice, remembered)}`
    : 'Forget MCU';
}

async function findRememberedMcuDevices() {
  const remembered = rememberedMcuDevice();
  const devices = [];
  if (selectedMcuDevice) devices.push(selectedMcuDevice);

  if (navigator.bluetooth?.getDevices) {
    const grantedDevices = await navigator.bluetooth.getDevices();
    for (const device of grantedDevices) {
      const sameDevice = remembered?.id && device.id === remembered.id;
      if (sameDevice || isLikelyMcuDevice(device)) devices.push(device);
    }
  }

  return [...new Map(devices.map(device => [device.id || device.name, device])).values()];
}

async function forgetMcuAssociation() {
  if (!('bluetooth' in navigator)) {
    showResult('mcuBleResult', 'error', 'Web Bluetooth is not available in this browser.');
    return;
  }

  busy = true;
  updateSteps();
  document.getElementById('mcuForgetBtnText').innerHTML = '<span class="spinner"></span> Forgetting…';

  try {
    const devices = await findRememberedMcuDevices();
    const forgettable = devices.filter(device => typeof device.forget === 'function');

    if (!devices.length) {
      localStorage.removeItem(MCU_DEVICE_STORAGE_KEY);
      selectedMcuDevice = null;
      showResult('mcuBleResult', 'info',
        `<div class="result-label">MCU Association</div>` +
        `<div class="result-value">No MCU device is remembered by this page. If Chrome still shows an old name, remove it from browser Bluetooth settings.</div>`
      );
      return;
    }

    if (!forgettable.length) {
      showResult('mcuBleResult', 'error',
        `<div class="result-label">Forget Unsupported</div>` +
        `<div class="result-value">This Chrome build exposes the device but not BluetoothDevice.forget(). Remove it from chrome://settings/content/bluetoothDevices.</div>`
      );
      return;
    }

    for (const device of forgettable) {
      if (device.gatt?.connected) device.gatt.disconnect();
      await device.forget();
    }

    localStorage.removeItem(MCU_DEVICE_STORAGE_KEY);
    selectedMcuDevice = null;
    showResult('mcuBleResult', 'success',
      `<div class="result-label">MCU Association Removed</div>` +
      `<div class="result-value">Forgot ${forgettable.length} MCU Bluetooth device${forgettable.length === 1 ? '' : 's'} for this site. Click Generate MCU Proof &amp; Send to choose it again.</div>`
    );
  } catch (error) {
    showResult('mcuBleResult', 'error',
      `<div class="result-label">Forget Failed</div>` +
      `<div class="result-value">${escapeHtml(String(error))}</div>`
    );
  } finally {
    busy = false;
    updateSteps();
  }
}

function base64ToBytes(value) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

// Populate the public-values editor with the hex from freshly generated artifacts.
// Stores original value so we can detect modifications.
function populateMcuPvEditor(hex) {
  const input = document.getElementById('mcuPvHex');
  const resetBtn = document.getElementById('mcuPvReset');
  if (!input) return;
  input.value = hex;
  input.dataset.original = hex;
  input.removeAttribute('readonly');
  if (resetBtn) resetBtn.style.display = 'none';
}

// Return the current hex value in the editor, or null if no editor / no value.
function getMcuPvEdited() {
  const input = document.getElementById('mcuPvHex');
  if (!input || input.hasAttribute('readonly')) return null;
  return input.value.trim().toLowerCase();
}

async function writeUtf8(characteristic, value, options) {
  await writeBleValue(characteristic, new TextEncoder().encode(value), options);
}

async function writeBleValue(characteristic, value, options = {}) {
  const { requireResponse = false, preferNoResponse = false } = options;

  const canWriteWithResponse = !!characteristic.properties?.write;
  const canWriteWithoutResponse = !!characteristic.properties?.writeWithoutResponse;

  if (preferNoResponse && canWriteWithoutResponse && 'writeValueWithoutResponse' in characteristic) {
    await characteristic.writeValueWithoutResponse(value);
    return;
  }

  if (requireResponse && canWriteWithResponse) {
    try {
      if ('writeValueWithResponse' in characteristic) {
        await characteristic.writeValueWithResponse(value);
      } else {
        await characteristic.writeValue(value);
      }
      return;
    } catch (error) {
      if (!(canWriteWithoutResponse && 'writeValueWithoutResponse' in characteristic && isBleWriteFallbackError(error))) {
        throw error;
      }
      await characteristic.writeValueWithoutResponse(value);
      return;
    }
  }

  if (canWriteWithoutResponse && 'writeValueWithoutResponse' in characteristic) {
    await characteristic.writeValueWithoutResponse(value);
  } else if (canWriteWithResponse && 'writeValueWithResponse' in characteristic) {
    await characteristic.writeValueWithResponse(value);
  } else {
    await characteristic.writeValue(value);
  }
}

async function sendBleBlob(characteristic, kind, bytes, chunkBytes, options = {}) {
  const { requireResponse = false, preferNoResponse = false, onProgress = null, yieldEveryPackets = 0 } = options;
  const payloadBytes = Math.max(20, Math.min(chunkBytes || 180, 512) - 4);
  let sequence = 0;
  for (let offset = 0; offset < bytes.length; offset += payloadBytes) {
    const slice = bytes.subarray(offset, Math.min(offset + payloadBytes, bytes.length));
    const packet = new Uint8Array(4 + slice.length);
    packet[0] = kind;
    packet[1] = sequence & 0xff;
    packet[2] = (sequence >> 8) & 0xff;
    packet[3] = offset + slice.length >= bytes.length ? 1 : 0;
    packet.set(slice, 4);
    await writeBleValue(characteristic, packet, { requireResponse, preferNoResponse });
    if (onProgress) onProgress(slice.length);
    sequence++;
    if (yieldEveryPackets > 0 && sequence % yieldEveryPackets === 0) {
      await new Promise(resolve => setTimeout(resolve, 0));
    }
  }
}

async function sendBleBlobWithBackpressure(characteristic, kind, bytes, chunkBytes, options = {}) {
  const {
    totalBytes,
    baseReceivedBytes = 0,
    statusChar,
    statusUpdates,
    onStatusMessage,
    onProgress = null,
  } = options;
  const payloadBytes = Math.max(20, Math.min(chunkBytes || 180, 512) - 4);
  const windowBytes = Math.max(payloadBytes * 2, Math.ceil((totalBytes || bytes.length) * 0.05));
  let sequence = 0;
  let sentBytes = 0;

  for (let offset = 0; offset < bytes.length; offset += payloadBytes) {
    const slice = bytes.subarray(offset, Math.min(offset + payloadBytes, bytes.length));
    const packet = new Uint8Array(4 + slice.length);
    packet[0] = kind;
    packet[1] = sequence & 0xff;
    packet[2] = (sequence >> 8) & 0xff;
    packet[3] = offset + slice.length >= bytes.length ? 1 : 0;
    packet.set(slice, 4);
    await writeBleValue(characteristic, packet, { requireResponse: true });
    sentBytes += slice.length;
    if (onProgress) onProgress(slice.length);
    sequence++;

    if (sentBytes > windowBytes) {
      const targetReceivedBytes = baseReceivedBytes + sentBytes - windowBytes;
      const receiveStatus = await waitForMcuStatus(
        statusChar,
        statusUpdates,
        message => {
          if (message.startsWith('error')) return true;
          const receiveProgress = parseMcuReceiveProgress(message);
          return !!receiveProgress && receiveProgress.receivedBytes >= targetReceivedBytes;
        },
        onStatusMessage,
        'Timed out waiting for ZKMCU receive progress',
        25
      );
      if (receiveStatus.startsWith('error')) {
        throw new Error(receiveStatus || 'ZKMCU reported an upload error');
      }
    }
  }
}

function isBleWriteFallbackError(error) {
  const name = error?.name || '';
  const message = String(error?.message || error || '');
  return name === 'NotSupportedError'
    || name === 'InvalidStateError'
    || /GATT operation failed/i.test(message);
}

async function waitForMcuStatus(statusChar, statusUpdates, predicate, onMessage, timeoutMessage, pollMs = 1500) {
  const deadline = Date.now() + 20 * 60 * 1000;
  while (Date.now() < deadline) {
    const latest = statusUpdates[statusUpdates.length - 1];
    if (latest && predicate(latest)) return latest;
    try {
      const value = await statusChar.readValue();
      const message = onMessage(decodeBleText(value));
      if (message && predicate(message)) return message;
    } catch (_) {
      // Notifications may be the only supported status path on some browsers.
    }
    await new Promise(resolve => setTimeout(resolve, pollMs));
  }
  throw new Error(timeoutMessage);
}

async function waitForMcuVerdict(statusChar, statusUpdates, onMessage) {
  return waitForMcuStatus(
    statusChar,
    statusUpdates,
    message => message.startsWith('verified') || message.startsWith('rejected') || message.startsWith('error'),
    onMessage,
    'Timed out waiting for ZKMCU verification result'
  );
}

function decodeBleText(value) {
  return new TextDecoder()
    .decode(new Uint8Array(value.buffer, value.byteOffset, value.byteLength))
    .replace(/\0+$/, '');
}

function statusType(message) {
  if (message.startsWith('verified')) return 'success';
  if (message.startsWith('rejected') || message.startsWith('error')) return 'error';
  return 'info';
}

function parseMcuReceiveProgress(message) {
  const match = /\brx\s+(\d+)%\s+(\d+)\/(\d+)B\b/.exec(message);
  if (!match) return null;
  const percent = Number(match[1]);
  const receivedBytes = Number(match[2]);
  const totalBytes = Number(match[3]);
  if (!Number.isFinite(percent) || !Number.isFinite(receivedBytes) || !Number.isFinite(totalBytes) || totalBytes <= 0) {
    return null;
  }
  return {
    percent: Math.max(0, Math.min(100, percent)),
    receivedBytes: Math.max(0, receivedBytes),
    totalBytes,
  };
}

// Ordered list of verification steps emitted by the MCU firmware.
// Each entry: [label, relative weight for progress bar estimation]
const MCU_VERIFY_STEPS = [
  ['transcript',   0.03],
  ['lagrange',     0.05],
  ['constraints',  0.10],
  ['commitments',  0.42], // heaviest — MSMs
  ['queries',      0.08],
  ['shplonk',      0.14],
  ['pairing',      0.18], // ≥1 KZG pairings
];

// Parse "verifying N label" messages from the MCU progress channel.
function parseMcuVerifyStep(message) {
  const match = /^verifying (\d+) ([a-z]+)$/.exec(message);
  if (!match) return null;
  const label = match[2];
  const idx = MCU_VERIFY_STEPS.findIndex(([l]) => l === label);
  if (idx === -1) return null;
  return { stepNum: Number(match[1]), label, idx };
}

// Render a step breadcrumb row showing which steps are done / active / pending.
function renderMcuVerifySteps(current) {
  return `<div class="mcu-verify-steps">` +
    MCU_VERIFY_STEPS.map(([label], idx) => {
      const state = !current ? 'pending'
        : idx < current.idx ? 'done'
        : idx === current.idx ? 'active'
        : 'pending';
      return `<span class="mcu-verify-step ${state}">${label}</span>`;
    }).join('') +
    `</div>`;
}

// Render an estimated-time progress bar for the MCU Halo2/KZG verification phase.
// elapsedMs: time since COMMIT was sent; estimatedMs: expected duration from prior run.
function renderMcuVerifyProgress(elapsedMs, estimatedMs) {
  const pct = Math.min(99, Math.round((elapsedMs / estimatedMs) * 100));
  const elapsedSec = (elapsedMs / 1000).toFixed(0);
  const remainSec = Math.max(0, Math.round((estimatedMs - elapsedMs) / 1000));
  const remainLabel = elapsedMs < estimatedMs
    ? `~${remainSec}s remaining`
    : `${((elapsedMs - estimatedMs) / 1000).toFixed(0)}s over estimate`;
  return `<div class="mcu-verify-progress" aria-label="MCU verify progress">` +
    `<div class="mcu-verify-track">` +
    `<div class="mcu-verify-fill" style="width:${pct}%"></div>` +
    `</div>` +
    `<div class="mcu-verify-labels">` +
    `<span>${elapsedSec}s elapsed</span>` +
    `<span>${pct}%</span>` +
    `<span>${remainLabel}</span>` +
    `</div>` +
    `</div>`;
}

function renderMcuReceiveProgress({ totalBytes, sentBytes = 0, receiveProgress = null }) {
  if (!totalBytes) return '';
  const effectiveTotal = receiveProgress?.totalBytes || totalBytes;
  const sentPercent = Math.max(0, Math.min(100, Math.round((sentBytes / totalBytes) * 100)));
  const receivePercent = receiveProgress?.percent ?? 0;
  const receivedBytes = receiveProgress?.receivedBytes ?? 0;

  return `<div class="mcu-transfer-progress" aria-label="MCU transfer progress">` +
    `<div class="mcu-transfer-row">` +
      `<span>Browser sent ${fmtBytes(sentBytes)} / ${fmtBytes(totalBytes)}</span>` +
      `<strong>${sentPercent}%</strong>` +
    `</div>` +
    `<div class="mcu-transfer-track browser"><div class="mcu-transfer-fill" style="width:${sentPercent}%"></div></div>` +
    `<div class="mcu-transfer-row">` +
      `<span>MCU received ${fmtBytes(receivedBytes)} / ${fmtBytes(effectiveTotal)}</span>` +
      `<strong>${receivePercent}%</strong>` +
    `</div>` +
    `<div class="mcu-transfer-track mcu"><div class="mcu-transfer-fill" style="width:${receivePercent}%"></div></div>` +
  `</div>`;
}

// ——— Pipeline helpers ———

function setPipeActive(step) {
  const el = document.getElementById(`pipeStep${step}`);
  el.classList.remove('done', 'fail');
  el.classList.add('active');
}

function setPipeFail(step) {
  const el = document.getElementById(`pipeStep${step}`);
  el.classList.remove('active', 'done');
  el.classList.add('fail');
}

// ——— Backend Banner ———

function showBackendBanner() {
  const banner = document.getElementById('backendBanner');
  if (banner) banner.classList.add('visible');
}

function hideBackendBanner() {
  const banner = document.getElementById('backendBanner');
  if (banner) banner.classList.remove('visible');
}

function updateProofUIVisibility() {
  const isBackendAvailable = backendStatus === 'available';
  
  // Update button text and title
  const btnText = document.getElementById('evalProveBtnText');
  const step2Title = document.getElementById('step2Title');
  if (btnText) {
    const newText = isBackendAvailable ? 'Evaluate &amp; Prove' : 'Evaluate';
    if (btnText.textContent === 'Evaluate & Prove' || btnText.innerHTML.includes('Evaluate')) {
      btnText.innerHTML = newText;
    }
  }
  if (step2Title) {
    step2Title.innerHTML = isBackendAvailable ? 'Evaluate &amp; Prove' : 'Evaluate';
  }
  
  // Hide/show proof-related elements
  const proofElements = [
    'step2Subtitle',     // "Evaluate UPLC locally, then generate a STARK proof on the server"
    'proofBadge',        // Remote badge
    'proveResult',       // Proof result box
    'step2Aside',        // Aside text about zkVM
    'step3Row',          // Commitment verification step
    'step4Row'           // STARK verification step
  ];
  
  proofElements.forEach(id => {
    const el = document.getElementById(id);
    if (el) {
      el.style.display = isBackendAvailable ? '' : 'none';
    }
  });
}

function getEvalButtonText() {
  return backendAvailable ? 'Evaluate &amp; Prove' : 'Evaluate';
}

// ——— Helpers ———

function showResult(id, type, html) {
  const el = document.getElementById(id);
  el.className = `result-box visible ${type}`;
  el.innerHTML = html;
}

function showVerdict(pass) {
  const v = document.getElementById('verdict');
  v.className = `verdict visible ${pass ? 'pass' : 'fail'}`;
  v.innerHTML = pass
    ? `<div class="verdict-icon">&#x2705;</div><h3>Verification Passed</h3><p>The UPLC program was honestly evaluated inside the OpenVM zkVM.<br><span style="font-size:0.75rem;color:var(--text-muted)">Evaluation, commitment checking, and final STARK verification ran locally in your browser. Only proof generation used the backend.</span></p>`
    : `<div class="verdict-icon">&#x274C;</div><h3>Verification Failed</h3><p>The proof could not be verified.</p>`;
}

function escapeHtml(s) {
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function fmtBytes(n) {
  if (n < 1024) return n + ' B';
  if (n < 1048576) return (n/1024).toFixed(1) + ' KB';
  return (n/1048576).toFixed(1) + ' MB';
}

async function buildProofDetails(data, proofJsonSize, baselineJsonSize) {
  const proofJson = JSON.stringify(data.stark_proof_json);
  const proofHash = await sha256Hex(proofJson);
  const userPublicValues = normalizePublicValuesHex(
    data.stark_proof_json?.user_public_values || data.commitment
  );
  const publicValueBytes = userPublicValues ? userPublicValues.length / 2 : 0;

  return {
    system: 'OpenVM STARK',
    openvm: data.openvm_version || 'unknown',
    version: data.proof_version || data.stark_proof_json?.version || 'unknown',
    proofJsonSize,
    baselineJsonSize,
    proofHash,
    publicValueBytes: `${publicValueBytes} bytes`,
    verifier: 'pending',
    verifiedIn: '-',
  };
}

function renderProofDetails() {
  const panel = document.getElementById('proofDetailPanel');
  const grid = document.getElementById('proofDetailGrid');
  if (!panel || !grid) return;

  if (!lastProofDetails) {
    panel.hidden = true;
    grid.innerHTML = '';
    return;
  }

  panel.hidden = false;
  const rows = [
    ['Proof system', lastProofDetails.system],
    ['OpenVM', lastProofDetails.openvm],
    ['Version', lastProofDetails.version],
    ['Proof JSON', lastProofDetails.proofJsonSize],
    ['Baseline', lastProofDetails.baselineJsonSize],
    ['Public values', lastProofDetails.publicValueBytes],
    ['Proof SHA-256', shortHex(lastProofDetails.proofHash)],
    ['Verifier', lastProofDetails.verifier],
    ['Time', lastProofDetails.verifiedIn],
  ];

  grid.innerHTML = rows.map(([label, value]) =>
    `<div class="proof-detail-label">${escapeHtml(label)}</div>` +
    `<div class="proof-detail-value">${escapeHtml(String(value))}</div>`
  ).join('');
}

async function sha256Hex(value) {
  const bytes = new TextEncoder().encode(value);
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('');
}

function shortHex(hex) {
  if (!hex || hex.length <= 24) return hex || 'unknown';
  return `${hex.slice(0, 12)}...${hex.slice(-12)}`;
}

// ——— Aiken editor highlighting ———

function syncHighlight() {
  const src = document.getElementById('aikenSource').value;
  // Append a newline so the <pre> always has room for the last line
  document.getElementById('aikenHighlight').innerHTML = highlightAiken(src) + '\n';
}

function syncScroll() {
  const ta = document.getElementById('aikenSource');
  const pre = document.getElementById('aikenHighlight').parentElement;
  pre.scrollTop = ta.scrollTop;
  pre.scrollLeft = ta.scrollLeft;
}

// ——— Init ———

const aikenTA = document.getElementById('aikenSource');
aikenTA.addEventListener('input', () => {
  aikenCompiled = false;
  resetFrom(1);
  syncHighlight();
});
aikenTA.addEventListener('scroll', syncScroll);

// Handle Tab key for indentation
aikenTA.addEventListener('keydown', (e) => {
  if (e.key === 'Tab') {
    e.preventDefault();
    const start = aikenTA.selectionStart;
    const end = aikenTA.selectionEnd;
    aikenTA.value = aikenTA.value.substring(0, start) + '  ' + aikenTA.value.substring(end);
    aikenTA.selectionStart = aikenTA.selectionEnd = start + 2;
    syncHighlight();
  }
});

// Initial highlight
syncHighlight();

document.getElementById('programHex').addEventListener('input', () => {
  const hexValue = document.getElementById('programHex').value.trim();
  resetFrom(1);
  // Enable toggle button only if we're on the uplcHex tab and hex is provided
  if (activeTab === 'uplcHex') {
    document.getElementById('toggleUplcPreview').disabled = !hexValue;
  }
  updateUplcDisplay();  // Update UPLC display when hex changes
  updateSteps();
});

// Button event listeners
document.getElementById('tabBtnAiken').addEventListener('click', () => switchTab('aiken'));
document.getElementById('tabBtnUplcHex').addEventListener('click', () => switchTab('uplcHex'));
document.getElementById('toggleUplcPreview').addEventListener('click', toggleUplcPreview);
document.getElementById('compileBtn').addEventListener('click', compileAiken);
document.getElementById('evalProveBtn').addEventListener('click', runEvaluateAndProve);
document.getElementById('commitBtn').addEventListener('click', runCommitmentCheck);
document.getElementById('starkBtn').addEventListener('click', runStarkVerification);
document.getElementById('mcuBleBtn').addEventListener('click', runMcuBleVerification);
document.getElementById('mcuForgetBtn').addEventListener('click', forgetMcuAssociation);
document.getElementById('bannerCloseBtn')?.addEventListener('click', hideBackendBanner);

// Public values editor: reset button + modified indicator
document.getElementById('mcuPvReset')?.addEventListener('click', () => {
  const input = document.getElementById('mcuPvHex');
  if (input) {
    input.value = input.dataset.original || '';
    input.classList.remove('pv-modified');
    document.getElementById('mcuPvReset').style.display = 'none';
  }
});
document.getElementById('mcuPvHex')?.addEventListener('input', () => {
  const input = document.getElementById('mcuPvHex');
  const resetBtn = document.getElementById('mcuPvReset');
  if (!input || !resetBtn) return;
  const changed = input.value.trim().toLowerCase() !== (input.dataset.original || '').toLowerCase();
  input.classList.toggle('pv-modified', changed);
  resetBtn.style.display = changed ? '' : 'none';
});

loadUplcWasm();
loadAikenWasm();
loadOpenVmVerifierWasm();
updateProofUIVisibility();  // Set initial visibility based on backendStatus
checkBackend();
updateSteps();
