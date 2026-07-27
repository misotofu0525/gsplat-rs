export const BENCHMARK_WINDOW_MODES = Object.freeze({
  currentStatsEvidence: "current_stats_evidence_window",
  terminalQueueThroughput: "terminal_queue_throughput_window",
});

function nonEmptyString(value, name) {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${name} must be a non-empty string`);
  }
  return value;
}

function sha256(value, name) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    throw new TypeError(`${name} must be a lowercase SHA-256 digest`);
  }
  return value;
}

function frameCount(value, name, allowZero = false) {
  if (!Number.isSafeInteger(value) || value < (allowZero ? 0 : 1)) {
    throw new TypeError(`${name} must be ${allowZero ? "non-negative" : "positive"}`);
  }
  return value;
}

function monotonicMs(value, name) {
  if (!Number.isFinite(value) || value < 0) {
    throw new TypeError(`${name} must be a finite non-negative monotonic timestamp`);
  }
  return value;
}

export function currentStatsEvidenceWindowIdentity({ runId, configurationSha256 }) {
  return Object.freeze({
    run_id: nonEmptyString(runId, "control artifact run id"),
    configuration_sha256: sha256(
      configurationSha256,
      "control artifact configuration SHA-256",
    ),
  });
}

export function currentStatsEvidenceWindowManifest({ runId, configurationSha256 }) {
  const controlArtifactIdentity = currentStatsEvidenceWindowIdentity({
    runId,
    configurationSha256,
  });
  return {
    mode: BENCHMARK_WINDOW_MODES.currentStatsEvidence,
    evidence_role: "renderer_exact_current_stats_control",
    performance_evidence: false,
    performance_exclusion_reason: "per_frame_current_stats_observer_load",
    current_stats_policy: "one_issued_terminal_pair_per_logical_frame",
    terminal_policy: "drain_all_issued_current_stats_after_final_submit",
    configuration_sha256: configurationSha256,
    control_artifact_identity: controlArtifactIdentity,
  };
}

export function validateBenchmarkWindowManifest({
  window,
  currentStatsSubmissionCount,
  expectedLogicalFrameCount,
  expectedWarmupFrameCount = 0,
  expectedConfigurationSha256,
}) {
  if (!window || typeof window !== "object" || Array.isArray(window)) {
    throw new TypeError("benchmark window manifest evidence is required");
  }
  frameCount(currentStatsSubmissionCount, "current-stats submission count", true);
  frameCount(expectedLogicalFrameCount, "expected logical frame count");
  frameCount(expectedWarmupFrameCount, "expected warmup frame count", true);
  sha256(expectedConfigurationSha256, "expected configuration SHA-256");
  if (window.configuration_sha256 !== expectedConfigurationSha256) {
    throw new Error("benchmark window configuration identity mismatch");
  }

  if (window.mode === BENCHMARK_WINDOW_MODES.currentStatsEvidence) {
    if (window.performance_evidence !== false
        || window.evidence_role !== "renderer_exact_current_stats_control"
        || window.performance_exclusion_reason !== "per_frame_current_stats_observer_load"
        || currentStatsSubmissionCount !== expectedLogicalFrameCount) {
      throw new Error(
        "current-stats evidence window cannot be admitted as cross-implementation performance evidence",
      );
    }
    const identity = currentStatsEvidenceWindowIdentity({
      runId: window.control_artifact_identity?.run_id,
      configurationSha256: window.control_artifact_identity?.configuration_sha256,
    });
    if (identity.run_id !== window.control_artifact_identity.run_id
        || identity.configuration_sha256 !== expectedConfigurationSha256) {
      throw new Error("current-stats control artifact identity mismatch");
    }
    return window;
  }

  if (window.mode === BENCHMARK_WINDOW_MODES.terminalQueueThroughput) {
    const expectedCurrentStatsCount = expectedWarmupFrameCount > 0 ? 2 : 1;
    if (window.performance_evidence !== true
        || window.evidence_role !== "cross_implementation_terminal_queue_throughput"
        || window.current_stats_policy
          !== "one_untimed_warmup_boundary_and_one_final_measured_receipt"
        || currentStatsSubmissionCount !== expectedCurrentStatsCount) {
      throw new Error(
        "terminal-queue throughput requires its untimed warmup boundary and final receipt",
      );
    }
    const control = currentStatsEvidenceWindowIdentity({
      runId: window.control_artifact_identity?.run_id,
      configurationSha256: window.control_artifact_identity?.configuration_sha256,
    });
    if (control.configuration_sha256 !== expectedConfigurationSha256) {
      throw new Error("throughput window is not bound to a same-configuration control artifact");
    }
    const warmupReceiptValid = expectedWarmupFrameCount === 0
      ? window.warmup_terminal_receipt_submission_count === 0
        && window.warmup_terminal_receipt_terminal_count === 0
        && window.warmup_terminal_receipt === null
        && window.warmup_boundary_current_stats_ticket === null
        && window.draw_count_at_warmup_drain_start === null
        && window.draw_count_at_warmup_drain_completion === null
      : window.warmup_terminal_receipt_submission_count === 1
        && window.warmup_terminal_receipt_terminal_count === 1
        && Number.isSafeInteger(window.warmup_boundary_current_stats_ticket)
        && window.warmup_boundary_current_stats_ticket > 0
        && window.warmup_terminal_receipt?.phase === "warmup_boundary"
        && window.warmup_terminal_receipt?.status === "ready"
        && window.warmup_terminal_receipt?.ticket
          === window.warmup_boundary_current_stats_ticket
        && window.warmup_terminal_receipt?.ticket
          !== window.final_measured_current_stats_ticket
        && window.draw_count_at_warmup_drain_start === expectedWarmupFrameCount
        && window.draw_count_at_warmup_drain_completion === expectedWarmupFrameCount
        && window.warmup_terminal_receipt.requested_at_monotonic_ms
          <= window.warmup_terminal_receipt.submitted_at_monotonic_ms
        && window.warmup_terminal_receipt.submitted_at_monotonic_ms
          <= window.warmup_terminal_receipt.terminal_at_monotonic_ms
        && window.warmup_terminal_receipt.terminal_at_monotonic_ms
          <= window.first_measured_input_monotonic_ms;
    const warmupOverheadValid = expectedWarmupFrameCount === 0
      ? window.terminal_receipt_overhead?.warmup_terminal_boundary
          === "not_applicable_no_warmup"
        && window.terminal_receipt_overhead?.residual_warmup_queue_tail
          === "not_applicable"
      : window.terminal_receipt_overhead?.warmup_terminal_boundary
          === "same_submission_result_ready_before_first_measured_input"
        && window.terminal_receipt_overhead?.residual_warmup_queue_tail === "excluded";
    const directQueueCompletion = window.completion_primitive
      === "gpu_queue_on_submitted_work_done";
    const terminalPrimitiveValid = directQueueCompletion
      ? window.queue_completion_timestamp_source === "wgpu_queue_callback_performance_now"
        && window.terminal_receipt_overhead?.kind
          === "renderer_current_stats_map_plus_direct_queue_callback_v2"
        && window.terminal_receipt_overhead?.queue_completion_callback === true
        && window.terminal_receipt_overhead?.fairness_assessment
          === "same_queue_completion_primitive"
      : window.completion_primitive === "renderer_current_stats_poll_observed"
        && window.queue_completion_timestamp_source
          === "raf_current_stats_poll_performance_now"
        && window.terminal_receipt_overhead?.kind
          === "renderer_current_stats_same_submission_map_v1"
        && window.terminal_receipt_overhead?.queue_completion_callback === false
        && window.terminal_receipt_overhead?.fairness_assessment
          === "conservative_nonidentical_terminal_proof_overhead_disclosed";
    if (window.warmup_submit_count !== expectedWarmupFrameCount
        || window.measured_submit_count !== expectedLogicalFrameCount
        || window.measured_wait_count_before_final_submit !== 0
        || window.terminal_current_stats_submission_count !== 1
        || window.terminal_current_stats_terminal_count !== 1
        || !warmupReceiptValid
        || window.draw_count_at_final_drain_start !== window.draw_count_at_completion
        || window.exact_adaptive_measured?.length !== expectedLogicalFrameCount
        || window.exact_adaptive_measured.some((record) =>
          window.execution_cell === "fixed_gpu_preproject_compact"
            ? record.state !== "disabled" || record.plan !== "gpu_preproject"
              || record.projected_state !== "disabled"
              || record.projected_execution !== "compact"
            : record.state === "disabled"
              || !["cpu_post_sort", "gpu_post_sort", "gpu_preproject"].includes(record.plan),
        )
        || window.terminal_receipt?.phase !== "final_measured"
        || window.terminal_receipt?.status !== "ready"
        || !Number.isSafeInteger(window.final_measured_current_stats_ticket)
        || window.final_measured_current_stats_ticket <= 0
        || window.terminal_receipt?.ticket !== window.final_measured_current_stats_ticket
        || window.terminal_receipt?.requested_at_monotonic_ms
          > window.terminal_receipt?.submitted_at_monotonic_ms
        || window.terminal_receipt?.submitted_at_monotonic_ms
          !== window.last_measured_submit_monotonic_ms
        || window.terminal_receipt?.terminal_at_monotonic_ms
          !== window.last_measured_terminal_monotonic_ms
        || window.final_queue_terminal_monotonic_ms
          !== window.last_measured_terminal_monotonic_ms
        || window.draw_count_at_final_drain_start
          !== expectedWarmupFrameCount + expectedLogicalFrameCount
        || !terminalPrimitiveValid
        || window.terminal_receipt_overhead?.readback_buffer_bytes !== 8
        || ![4, 8].includes(window.terminal_receipt_overhead?.encoded_copy_bytes)
        || window.terminal_receipt_overhead?.extra_queue_submissions !== 0
        || window.terminal_receipt_overhead?.map_async_result_required !== true
        || window.terminal_receipt_overhead?.included_in_terminal_window !== true
        || !warmupOverheadValid
        || window.terminal_receipt_overhead?.competitor_terminal_primitive
          !== "queue_on_submitted_work_done_promise"
        || !Number.isFinite(window.first_measured_input_monotonic_ms)
        || !Number.isFinite(window.first_measured_submit_monotonic_ms)
        || !Number.isFinite(window.last_measured_submit_monotonic_ms)
        || window.first_measured_input_monotonic_ms
          > window.first_measured_submit_monotonic_ms
        || window.first_measured_submit_monotonic_ms
          > window.last_measured_submit_monotonic_ms
        || window.last_measured_submit_monotonic_ms
          > window.last_measured_terminal_monotonic_ms
        || window.input_to_first_submit_ms
          !== window.first_measured_submit_monotonic_ms
            - window.first_measured_input_monotonic_ms
        || window.submit_span_ms
          !== window.last_measured_submit_monotonic_ms
            - window.first_measured_submit_monotonic_ms
        || window.terminal_tail_ms
          !== window.last_measured_terminal_monotonic_ms
            - window.last_measured_submit_monotonic_ms
        || window.terminal_window_ms
          !== window.last_measured_terminal_monotonic_ms
            - window.first_measured_input_monotonic_ms
        || window.terminal_window_ms < 0) {
      throw new Error(
        "terminal-queue throughput window lacks continuous submits or its no-new-draw drain",
      );
    }
    return window;
  }

  throw new Error(`unknown benchmark window mode ${window.mode ?? "missing"}`);
}

export function createTerminalQueueThroughputWindow({
  warmupFrames,
  measuredFrames,
  configurationSha256,
  controlArtifactIdentity,
  executionCell = "adaptive",
  directQueueCompletion = false,
}) {
  const warmup = frameCount(warmupFrames, "warmup frame count", true);
  const measured = frameCount(measuredFrames, "measured frame count");
  const configuration = sha256(configurationSha256, "configuration SHA-256");
  const control = currentStatsEvidenceWindowIdentity({
    runId: controlArtifactIdentity?.run_id,
    configurationSha256: controlArtifactIdentity?.configuration_sha256,
  });
  if (control.configuration_sha256 !== configuration) {
    throw new Error("throughput window requires a same-configuration current-stats control");
  }
  if (!["adaptive", "fixed_gpu_preproject_compact"].includes(executionCell)) {
    throw new Error(`unknown terminal-queue execution cell ${executionCell}`);
  }

  let state = warmup === 0
    ? "measured_input_accept"
    : warmup === 1
      ? "warmup_receipt_request"
      : "warmup_submitting";
  let warmupSubmitted = 0;
  let measuredSubmitted = 0;
  let drawCount = 0;
  let drawCountAtWarmupDrain = null;
  let drawCountAtWarmupDrainCompletion = null;
  let drawCountAtFinalDrain = null;
  let firstMeasuredInputMs = null;
  let firstMeasuredSubmitMs = null;
  let lastMeasuredSubmitMs = null;
  let finalTerminalMs = null;
  let warmupReceiptRequestMs = null;
  let warmupBoundarySubmitMs = null;
  let finalReceiptRequestMs = null;
  let warmupBoundaryCurrentStatsTicket = null;
  let warmupBoundaryPlan = null;
  let finalMeasuredCurrentStatsTicket = null;
  let finalMeasuredPlan = null;
  let warmupTerminalReceipt = null;
  let finalTerminalReceipt = null;
  const measuredExactAdaptive = [];

  return {
    get state() {
      return state;
    },
    get action() {
      if (state.endsWith("submitting")) return "draw";
      if (state === "warmup_receipt_request") return "request_warmup_receipt";
      if (state === "warmup_receipt_poll") return "poll_warmup_receipt";
      if (state === "measured_input_accept") return "accept_measured_input";
      if (state === "final_receipt_request") return "request_final_receipt";
      if (state === "final_receipt_poll") return "poll_final_receipt";
      return "complete";
    },

    beginWarmupReceipt({ requestedAtMonotonicMs }) {
      monotonicMs(requestedAtMonotonicMs, "warmup receipt request timestamp");
      if (state !== "warmup_receipt_request") {
        throw new Error(`unexpected warmup receipt request while ${state}`);
      }
      warmupReceiptRequestMs = requestedAtMonotonicMs;
      state = "warmup_terminal_submitting";
    },

    beginMeasuredInput({ acceptedAtMonotonicMs }) {
      monotonicMs(acceptedAtMonotonicMs, "first measured input timestamp");
      if (state !== "measured_input_accept") {
        throw new Error(`unexpected first measured input while ${state}`);
      }
      if (warmupTerminalReceipt !== null
          && acceptedAtMonotonicMs < warmupTerminalReceipt.terminal_at_monotonic_ms) {
        throw new Error("first measured input precedes the warmup queue terminal");
      }
      firstMeasuredInputMs = acceptedAtMonotonicMs;
      state = measured === 1 ? "final_receipt_request" : "measured_submitting";
    },

    beginFinalReceipt({ requestedAtMonotonicMs }) {
      monotonicMs(requestedAtMonotonicMs, "final receipt request timestamp");
      if (state !== "final_receipt_request") {
        throw new Error(`unexpected final receipt request while ${state}`);
      }
      if (firstMeasuredInputMs === null || requestedAtMonotonicMs < firstMeasuredInputMs) {
        throw new Error("final receipt request precedes the first measured input");
      }
      finalReceiptRequestMs = requestedAtMonotonicMs;
      state = "terminal_measured_submitting";
    },

    noteDraw({
      currentStatsSubmission,
      currentStatsTicket,
      submittedAtMonotonicMs,
      projectedAdaptiveState,
      projectedExecution,
      exactAdaptiveState,
      actualPlan,
    }) {
      monotonicMs(submittedAtMonotonicMs, "throughput submit timestamp");
      if (!state.endsWith("submitting")) {
        throw new Error(`terminal-queue throughput forbids draw while ${state}`);
      }
      const terminalWarmup = state === "warmup_terminal_submitting";
      const terminalMeasured = state === "terminal_measured_submitting";
      if (terminalWarmup || terminalMeasured) {
        if (currentStatsSubmission !== "issued") {
          throw new Error(
            `${terminalWarmup ? "warmup boundary" : "final measured"} frame did not issue `
            + "its terminal current-stats receipt",
          );
        }
        frameCount(
          currentStatsTicket,
          `${terminalWarmup ? "warmup boundary" : "final measured"} current-stats ticket`,
        );
      } else if (currentStatsSubmission !== "not_requested" || currentStatsTicket !== null) {
        throw new Error("warmup or non-final measured frame requested current stats");
      }
      if (typeof projectedAdaptiveState !== "string" || projectedAdaptiveState.length === 0
          || !["candidate", "compact"].includes(projectedExecution)) {
        throw new Error("terminal-queue throughput lacks projected adaptive state/execution");
      }
      if (executionCell === "fixed_gpu_preproject_compact") {
        if (exactAdaptiveState !== "disabled" || actualPlan !== "gpu_preproject"
            || projectedAdaptiveState !== "disabled" || projectedExecution !== "compact") {
          throw new Error("fixed GPU preproject Compact cell execution drifted");
        }
      } else if (!["cpu_learning", "cpu_stable", "gpu_probe", "gpu_stable", "cpu_probe", "cooldown"]
        .includes(exactAdaptiveState)
          || !["cpu_post_sort", "gpu_post_sort", "gpu_preproject"].includes(actualPlan)) {
        throw new Error("terminal-queue throughput lacks an active Exact adaptive state/plan");
      }
      drawCount += 1;
      if (state === "warmup_submitting") {
        warmupSubmitted += 1;
        if (warmupSubmitted === warmup - 1) {
          state = "warmup_receipt_request";
        }
        return "warmup";
      }
      if (terminalWarmup) {
        warmupSubmitted += 1;
        if (warmupSubmitted !== warmup) {
          throw new Error("warmup receipt was not attached to the final warmup frame");
        }
        warmupBoundaryCurrentStatsTicket = currentStatsTicket;
        warmupBoundaryPlan = actualPlan;
        warmupBoundarySubmitMs = submittedAtMonotonicMs;
        drawCountAtWarmupDrain = drawCount;
        state = "warmup_receipt_poll";
        return "warmup";
      }
      if (firstMeasuredInputMs === null || submittedAtMonotonicMs < firstMeasuredInputMs) {
        throw new Error("measured submission precedes the accepted first measured input");
      }
      if (firstMeasuredSubmitMs === null) {
        firstMeasuredSubmitMs = submittedAtMonotonicMs;
      }
      lastMeasuredSubmitMs = submittedAtMonotonicMs;
      measuredExactAdaptive.push({
        state: exactAdaptiveState,
        plan: actualPlan,
        projected_state: projectedAdaptiveState,
        projected_execution: projectedExecution,
      });
      measuredSubmitted += 1;
      if (terminalMeasured) {
        if (measuredSubmitted !== measured) {
          throw new Error("terminal current-stats receipt was not attached to the final measured frame");
        }
        finalMeasuredCurrentStatsTicket = currentStatsTicket;
        finalMeasuredPlan = actualPlan;
        if (finalMeasuredCurrentStatsTicket === warmupBoundaryCurrentStatsTicket) {
          throw new Error("warmup and final receipts reused one current-stats ticket");
        }
        state = "final_receipt_poll";
        drawCountAtFinalDrain = drawCount;
      } else if (measuredSubmitted === measured - 1) {
        state = "final_receipt_request";
      }
      return "measured";
    },

    recordWarmupReceipt({ ticket, status, plan, terminalAtMonotonicMs }) {
      if (state !== "warmup_receipt_poll") {
        throw new Error(`unexpected warmup receipt poll while ${state}`);
      }
      if (ticket !== warmupBoundaryCurrentStatsTicket) {
        throw new Error(
          `warmup receipt identity drift expected=${warmupBoundaryCurrentStatsTicket} `
          + `observed=${ticket}`,
        );
      }
      if (status !== "ready") {
        throw new Error(`warmup receipt ticket=${ticket} terminated with ${status ?? "missing"}`);
      }
      if (plan !== warmupBoundaryPlan) {
        throw new Error(
          `warmup receipt plan drift expected=${warmupBoundaryPlan} observed=${plan ?? "missing"}`,
        );
      }
      monotonicMs(terminalAtMonotonicMs, "warmup receipt timestamp");
      if (terminalAtMonotonicMs < warmupReceiptRequestMs
          || terminalAtMonotonicMs < warmupBoundarySubmitMs) {
        throw new Error("warmup receipt precedes its request or boundary submission");
      }
      warmupTerminalReceipt = {
        phase: "warmup_boundary",
        ticket,
        status,
        plan,
        requested_at_monotonic_ms: warmupReceiptRequestMs,
        submitted_at_monotonic_ms: warmupBoundarySubmitMs,
        terminal_at_monotonic_ms: terminalAtMonotonicMs,
      };
      drawCountAtWarmupDrainCompletion = drawCount;
      state = "measured_input_accept";
      return true;
    },

    recordTerminalReceipt({ ticket, status, plan, terminalAtMonotonicMs }) {
      if (state !== "final_receipt_poll") {
        throw new Error(`unexpected terminal receipt poll while ${state}`);
      }
      if (ticket !== finalMeasuredCurrentStatsTicket) {
        throw new Error(
          `terminal receipt identity drift expected=${finalMeasuredCurrentStatsTicket} `
          + `observed=${ticket}`,
        );
      }
      if (status !== "ready") {
        throw new Error(`terminal receipt ticket=${ticket} terminated with ${status ?? "missing"}`);
      }
      if (plan !== finalMeasuredPlan) {
        throw new Error(
          `terminal receipt plan drift expected=${finalMeasuredPlan} observed=${plan ?? "missing"}`,
        );
      }
      monotonicMs(terminalAtMonotonicMs, "terminal receipt timestamp");
      if (terminalAtMonotonicMs < finalReceiptRequestMs
          || terminalAtMonotonicMs < lastMeasuredSubmitMs) {
        throw new Error("terminal receipt precedes its request or final measured submission");
      }
      finalTerminalReceipt = {
        phase: "final_measured",
        ticket,
        status,
        plan,
        requested_at_monotonic_ms: finalReceiptRequestMs,
        submitted_at_monotonic_ms: lastMeasuredSubmitMs,
        terminal_at_monotonic_ms: terminalAtMonotonicMs,
      };
      finalTerminalMs = terminalAtMonotonicMs;
      if (finalTerminalMs < firstMeasuredInputMs) {
        throw new Error("final terminal receipt precedes first measured input");
      }
      state = "complete";
      return true;
    },

    evidence() {
      if (state !== "complete") {
        throw new Error(`terminal-queue throughput evidence is unavailable while ${state}`);
      }
      const window = {
        mode: BENCHMARK_WINDOW_MODES.terminalQueueThroughput,
        evidence_role: "cross_implementation_terminal_queue_throughput",
        performance_evidence: true,
        current_stats_policy:
          "one_untimed_warmup_boundary_and_one_final_measured_receipt",
        terminal_policy:
          "drain_warmup_before_first_measured_input_and_stop_after_final_measured_submit",
        configuration_sha256: configuration,
        control_artifact_identity: control,
        warmup_submit_count: warmupSubmitted,
        measured_submit_count: measuredSubmitted,
        measured_wait_count_before_final_submit: 0,
        warmup_terminal_receipt_submission_count: warmup > 0 ? 1 : 0,
        warmup_terminal_receipt_terminal_count: warmup > 0 ? 1 : 0,
        terminal_current_stats_submission_count: 1,
        terminal_current_stats_terminal_count: 1,
        warmup_boundary_current_stats_ticket: warmupBoundaryCurrentStatsTicket,
        final_measured_current_stats_ticket: finalMeasuredCurrentStatsTicket,
        draw_count_at_warmup_drain_start: drawCountAtWarmupDrain,
        draw_count_at_warmup_drain_completion: drawCountAtWarmupDrainCompletion,
        draw_count_at_final_drain_start: drawCountAtFinalDrain,
        draw_count_at_completion: drawCount,
        first_measured_input_monotonic_ms: firstMeasuredInputMs,
        first_measured_submit_monotonic_ms: firstMeasuredSubmitMs,
        last_measured_submit_monotonic_ms: lastMeasuredSubmitMs,
        final_queue_terminal_monotonic_ms: finalTerminalMs,
        last_measured_terminal_monotonic_ms: finalTerminalMs,
        input_to_first_submit_ms: firstMeasuredSubmitMs - firstMeasuredInputMs,
        submit_span_ms: lastMeasuredSubmitMs - firstMeasuredSubmitMs,
        terminal_tail_ms: finalTerminalMs - lastMeasuredSubmitMs,
        terminal_window_ms: finalTerminalMs - firstMeasuredInputMs,
        warmup_terminal_receipt: warmupTerminalReceipt === null
          ? null
          : { ...warmupTerminalReceipt },
        terminal_receipt: { ...finalTerminalReceipt },
        completion_primitive: directQueueCompletion
          ? "gpu_queue_on_submitted_work_done"
          : "renderer_current_stats_poll_observed",
        queue_completion_timestamp_source: directQueueCompletion
          ? "wgpu_queue_callback_performance_now"
          : "raf_current_stats_poll_performance_now",
        terminal_receipt_overhead: {
          kind: directQueueCompletion
            ? "renderer_current_stats_map_plus_direct_queue_callback_v2"
            : "renderer_current_stats_same_submission_map_v1",
          readback_buffer_bytes: 8,
          encoded_copy_bytes: finalMeasuredPlan === "cpu_post_sort" ? 4 : 8,
          extra_queue_submissions: 0,
          map_async_result_required: true,
          included_in_terminal_window: true,
          warmup_terminal_boundary: warmup > 0
            ? "same_submission_result_ready_before_first_measured_input"
            : "not_applicable_no_warmup",
          residual_warmup_queue_tail: warmup > 0 ? "excluded" : "not_applicable",
          competitor_terminal_primitive: "queue_on_submitted_work_done_promise",
          queue_completion_callback: directQueueCompletion,
          fairness_assessment: directQueueCompletion
            ? "same_queue_completion_primitive"
            : "conservative_nonidentical_terminal_proof_overhead_disclosed",
        },
        exact_adaptive_measured: measuredExactAdaptive.map((record) => ({ ...record })),
      };
      if (executionCell !== "adaptive") window.execution_cell = executionCell;
      return validateBenchmarkWindowManifest({
        window,
        currentStatsSubmissionCount: warmup > 0 ? 2 : 1,
        expectedLogicalFrameCount: measured,
        expectedWarmupFrameCount: warmup,
        expectedConfigurationSha256: configuration,
      });
    },
  };
}
