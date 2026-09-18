# Progress: Reduce Wasted Work

## 2026-09-18

- Branched `cursor/reduce-wasted-work-ac10` from PR #41 head `b45eb13`.
- Installed Mesa Lavapipe, fetched Kitsune, recorded baselines: bench-runner
  Kitsune/minimal, stability RSS, desktop PNG hashes, Xvfb viewer CPU time.
- SH early-out landed (`ad93c12`): PNG hashes identical; isolated projection
  pass on the culled-heavy default view 1.94 -> 1.25 ms.
- Stationary projection cache landed (`e4454eb`): re-projection only on
  params/order change; cached stationary frames 0.005 ms vs 4.2 ms.
- On-demand presentation landed (`6830b13`): `presented`, `needs_frame`,
  `request_present`; desktop viewer idles; stationary viewer 30.4 -> 0.1 CPU-s
  per 10 s.
- Async sort workspace reuse landed (`7948208`): worker-owned workspace,
  recycled index buffers, pointer-identity tests.
- CPU copy convergence landed (`7d03563`): derived arrays removed, mapped
  upload fill, `Arc<SceneBuffers>` shared with the worker; Kitsune RSS
  -17.3 MiB.
- Handbook / README updates landed (`7a77153`).
- Day-to-day verification set run on the final tree; results in the
  Project store report.
