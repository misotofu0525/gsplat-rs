export const Q1_HOST_START_RECEIPT_SCHEMA = "gsplat-q1-host-start/v1";

function finiteDimension(value, label) {
  if (!Number.isFinite(value) || value <= 0) {
    throw new TypeError(`${label} must be a positive finite number`);
  }
  return value;
}

export function captureQ1HostStartReceipt({
  documentObject = document,
  windowObject = window,
  canvas,
  expectedWidth,
  expectedHeight,
}) {
  if (!canvas || typeof canvas.getBoundingClientRect !== "function") {
    throw new TypeError("Q1 host start requires a canvas");
  }
  const rect = canvas.getBoundingClientRect();
  return {
    schema: Q1_HOST_START_RECEIPT_SCHEMA,
    phase: "host_start",
    expected_width: expectedWidth,
    expected_height: expectedHeight,
    inner_width: finiteDimension(windowObject.innerWidth, "inner width"),
    inner_height: finiteDimension(windowObject.innerHeight, "inner height"),
    visual_viewport_width: finiteDimension(
      windowObject.visualViewport?.width,
      "visual viewport width",
    ),
    visual_viewport_height: finiteDimension(
      windowObject.visualViewport?.height,
      "visual viewport height",
    ),
    canvas_css_width: finiteDimension(rect.width, "canvas CSS width"),
    canvas_css_height: finiteDimension(rect.height, "canvas CSS height"),
    canvas_backing_width: canvas.width,
    canvas_backing_height: canvas.height,
    device_pixel_ratio: windowObject.devicePixelRatio,
    visibility_state: documentObject.visibilityState,
    document_has_focus: documentObject.hasFocus(),
  };
}

export function validateQ1HostStartReceipt(receipt) {
  const width = receipt?.expected_width;
  const height = receipt?.expected_height;
  const exactDimensions = [
    [receipt?.inner_width, width],
    [receipt?.inner_height, height],
    [receipt?.visual_viewport_width, width],
    [receipt?.visual_viewport_height, height],
    [receipt?.canvas_css_width, width],
    [receipt?.canvas_css_height, height],
    [receipt?.canvas_backing_width, width],
    [receipt?.canvas_backing_height, height],
  ];
  if (receipt?.schema !== Q1_HOST_START_RECEIPT_SCHEMA
      || receipt.phase !== "host_start"
      || !Number.isSafeInteger(width) || width <= 0
      || !Number.isSafeInteger(height) || height <= 0
      || exactDimensions.some(([actual, expected]) => actual !== expected)
      || receipt.device_pixel_ratio !== 1
      || receipt.visibility_state !== "visible"
      || receipt.document_has_focus !== true) {
    throw new TypeError(
      `Q1 host start is not a visible focused full-resolution surface: ${JSON.stringify(receipt)}`,
    );
  }
  return receipt;
}

export function createQ1HostStartGate({ capture, start }) {
  if (typeof capture !== "function" || typeof start !== "function") {
    throw new TypeError("Q1 host start gate requires capture and start callbacks");
  }
  let state = "idle";
  let receipt = null;
  return {
    arm() {
      if (state !== "idle") throw new TypeError(`Q1 host start cannot arm from ${state}`);
      state = "armed";
      return { schema: Q1_HOST_START_RECEIPT_SCHEMA, state };
    },
    start() {
      if (state !== "armed") throw new TypeError(`Q1 host start cannot begin from ${state}`);
      try {
        receipt = validateQ1HostStartReceipt(capture());
        state = "starting";
        start();
        state = "started";
        return receipt;
      } catch (error) {
        state = "failed";
        throw error;
      }
    },
    status() {
      return { schema: Q1_HOST_START_RECEIPT_SCHEMA, state, receipt };
    },
  };
}
