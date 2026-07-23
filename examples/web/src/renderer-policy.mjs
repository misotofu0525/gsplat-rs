const ENABLED_VALUES = new Set(["1", "true", "yes"]);
const DISABLED_VALUES = new Set(["0", "false", "no"]);

export const SAMPLED_WEBGL_OPT_IN_PARAM = "gsplat_allow_sampled_webgl";

export function sampledWebglOptIn(searchParams) {
  const raw = searchParams.get(SAMPLED_WEBGL_OPT_IN_PARAM);
  if (raw == null || raw === "") return false;
  const normalized = raw.toLowerCase();
  if (ENABLED_VALUES.has(normalized)) return true;
  if (DISABLED_VALUES.has(normalized)) return false;
  throw new TypeError(`${SAMPLED_WEBGL_OPT_IN_PARAM} must be true or false`);
}

export function rendererFailurePolicy({ sampledWebglEnabled, formalEvidenceRequested }) {
  return sampledWebglEnabled && !formalEvidenceRequested
    ? "sampled_webgl_diagnostic"
    : "fail_closed";
}
