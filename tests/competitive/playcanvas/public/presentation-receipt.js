function finite(value, label) {
  const result = Number(value);
  if (!Number.isFinite(result)) throw new Error(`${label} must be finite`);
  return result;
}

function close(actual, expected, tolerance = 1e-6) {
  return Number.isFinite(actual) && Math.abs(actual - expected) <= tolerance;
}

export const VISUAL_VIEWPORT_QUANTIZATION_TOLERANCE_CSS_PX = 0.5;
export const PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX = 2;

export function shouldObserveMeasuredPresentation(capturePresentation) {
  return capturePresentation === true;
}

function elementDescriptor(element, canvas) {
  if (!element) return null;
  const ancestry = [];
  let current = element;
  for (let depth = 0; current && depth < 8; depth += 1) {
    ancestry.push({
      tag: current.tagName?.toLowerCase?.() ?? null,
      id: current.id || null,
      class_name: typeof current.className === 'string' ? current.className : null
    });
    current = current.parentElement;
  }
  return {
    tag: element.tagName?.toLowerCase?.() ?? null,
    id: element.id || null,
    class_name: typeof element.className === 'string' ? element.className : null,
    is_canvas: element === canvas,
    canvas_contains_hit: typeof canvas.contains === 'function' ? canvas.contains(element) : false,
    hit_contains_canvas: typeof element.contains === 'function' ? element.contains(canvas) : false,
    ancestry
  };
}

function canvasHitReceipt(documentObject, canvas, width, height, inset) {
  if (typeof documentObject.elementFromPoint !== 'function') {
    throw new Error('document.elementFromPoint is unavailable');
  }
  const samplePoints = [
    ['top_left', inset, inset],
    ['top_right', width - inset, inset],
    ['center', width / 2, height / 2],
    ['bottom_left', inset, height - inset],
    ['bottom_right', width - inset, height - inset]
  ];
  const samples = samplePoints.map(([name, x, y]) => {
    const hit = documentObject.elementFromPoint(x, y);
    const stack = typeof documentObject.elementsFromPoint === 'function'
      ? documentObject.elementsFromPoint(x, y)
      : [hit].filter(Boolean);
    return {
      name,
      x,
      y,
      hit: elementDescriptor(hit, canvas),
      hit_stack: stack.slice(0, 8).map((element) => elementDescriptor(element, canvas))
    };
  });
  return {
    inset_css_px: inset,
    probe_count: samples.length,
    all_samples_hit_canvas: samples.every((sample) => sample.hit?.is_canvas === true),
    samples
  };
}

function safeAreaInsets(documentObject, windowObject) {
  if (typeof documentObject.createElement !== 'function' || !documentObject.body ||
      typeof windowObject.getComputedStyle !== 'function') {
    return { observed: false };
  }
  const probe = documentObject.createElement('div');
  probe.style.cssText = [
    'position:fixed',
    'left:0',
    'top:0',
    'visibility:hidden',
    'pointer-events:none',
    'padding-top:env(safe-area-inset-top, 0px)',
    'padding-right:env(safe-area-inset-right, 0px)',
    'padding-bottom:env(safe-area-inset-bottom, 0px)',
    'padding-left:env(safe-area-inset-left, 0px)'
  ].join(';');
  documentObject.body.appendChild(probe);
  try {
    const style = windowObject.getComputedStyle(probe);
    const pixels = (value) => finite(Number.parseFloat(value || '0'), 'safe-area inset');
    return {
      observed: true,
      top: pixels(style.paddingTop),
      right: pixels(style.paddingRight),
      bottom: pixels(style.paddingBottom),
      left: pixels(style.paddingLeft)
    };
  } finally {
    probe.remove();
  }
}

export function captureBrowserPresentationState({
  documentObject = document,
  windowObject = window,
  canvas,
  expectedWidth,
  expectedHeight,
  phase,
  now = () => performance.now()
}) {
  if (!canvas || typeof canvas.getBoundingClientRect !== 'function') {
    throw new Error('presentation receipt requires a canvas element');
  }
  const visualViewport = windowObject.visualViewport;
  if (!visualViewport) throw new Error('visualViewport is unavailable');
  const rect = canvas.getBoundingClientRect();
  const unobscured = canvasHitReceipt(
    documentObject,
    canvas,
    rect.width,
    rect.height,
    1
  );
  const halfPixelEdgeDiagnostic = canvasHitReceipt(
    documentObject,
    canvas,
    rect.width,
    rect.height,
    0.5
  );
  const receipt = {
    phase,
    observed_at_ms: finite(now(), 'presentation observed_at_ms'),
    visibility_state: documentObject.visibilityState,
    hidden: documentObject.hidden,
    has_focus: documentObject.hasFocus(),
    device_pixel_ratio: finite(windowObject.devicePixelRatio, 'devicePixelRatio'),
    inner_width: finite(windowObject.innerWidth, 'innerWidth'),
    inner_height: finite(windowObject.innerHeight, 'innerHeight'),
    visual_viewport: {
      width: finite(visualViewport.width, 'visualViewport.width'),
      height: finite(visualViewport.height, 'visualViewport.height'),
      scale: finite(visualViewport.scale, 'visualViewport.scale'),
      offset_left: finite(visualViewport.offsetLeft, 'visualViewport.offsetLeft'),
      offset_top: finite(visualViewport.offsetTop, 'visualViewport.offsetTop'),
      dimension_tolerance_css_px: VISUAL_VIEWPORT_QUANTIZATION_TOLERANCE_CSS_PX
    },
    window_geometry: {
      outer_width: finite(windowObject.outerWidth ?? windowObject.innerWidth, 'outerWidth'),
      outer_height: finite(windowObject.outerHeight ?? windowObject.innerHeight, 'outerHeight'),
      screen_width: finite(windowObject.screen?.width ?? windowObject.innerWidth, 'screen.width'),
      screen_height: finite(windowObject.screen?.height ?? windowObject.innerHeight, 'screen.height'),
      screen_avail_width: finite(
        windowObject.screen?.availWidth ?? windowObject.innerWidth,
        'screen.availWidth'
      ),
      screen_avail_height: finite(
        windowObject.screen?.availHeight ?? windowObject.innerHeight,
        'screen.availHeight'
      ),
      orientation_type: windowObject.screen?.orientation?.type ?? null,
      orientation_angle: windowObject.screen?.orientation?.angle ?? null
    },
    safe_area_insets: safeAreaInsets(documentObject, windowObject),
    canvas: {
      backing_width: canvas.width,
      backing_height: canvas.height,
      css_left: finite(rect.left, 'canvas rect left'),
      css_top: finite(rect.top, 'canvas rect top'),
      css_width: finite(rect.width, 'canvas rect width'),
      css_height: finite(rect.height, 'canvas rect height')
    },
    canvas_unobscured: unobscured,
    canvas_half_pixel_edge_diagnostic: halfPixelEdgeDiagnostic,
    expected_width: expectedWidth,
    expected_height: expectedHeight,
    physical_pixel_mapping: {
      tolerance_px: PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX,
      viewport_width_px: visualViewport.width * windowObject.devicePixelRatio,
      viewport_height_px: visualViewport.height * windowObject.devicePixelRatio,
      canvas_css_width_px: rect.width * windowObject.devicePixelRatio,
      canvas_css_height_px: rect.height * windowObject.devicePixelRatio
    }
  };
  assertBrowserPresentationState(receipt);
  return receipt;
}

export function assertBrowserPresentationState(receipt) {
  const width = receipt?.expected_width;
  const height = receipt?.expected_height;
  const viewport = receipt?.visual_viewport;
  const canvas = receipt?.canvas;
  const mapping = receipt?.physical_pixel_mapping;
  const dpr = receipt?.device_pixel_ratio;
  const cssTolerance = VISUAL_VIEWPORT_QUANTIZATION_TOLERANCE_CSS_PX;
  const physicalTolerance = PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX;
  const valid = receipt?.visibility_state === 'visible' && receipt?.hidden === false &&
    receipt?.has_focus === true && Number.isFinite(dpr) && dpr > 0 &&
    viewport?.dimension_tolerance_css_px === cssTolerance &&
    close(receipt?.inner_width, viewport?.width, cssTolerance) &&
    close(receipt?.inner_height, viewport?.height, cssTolerance) &&
    close(viewport?.scale, 1) && close(viewport?.offset_left, 0) &&
    close(viewport?.offset_top, 0) && canvas?.backing_width === width &&
    canvas?.backing_height === height && close(canvas?.css_left, 0) &&
    close(canvas?.css_top, 0) && close(canvas?.css_width, viewport?.width, cssTolerance) &&
    close(canvas?.css_height, viewport?.height, cssTolerance) &&
    mapping?.tolerance_px === physicalTolerance &&
    close(mapping?.viewport_width_px, width, physicalTolerance) &&
    close(mapping?.viewport_height_px, height, physicalTolerance) &&
    close(mapping?.canvas_css_width_px, width, physicalTolerance) &&
    close(mapping?.canvas_css_height_px, height, physicalTolerance) &&
    receipt?.canvas_unobscured?.probe_count === 5 &&
    receipt?.canvas_unobscured?.all_samples_hit_canvas === true;
  if (!valid) {
    throw new Error(
      `${receipt?.phase ?? 'unknown'}: browser page is not a focused, visible, ` +
      `native-DPR full-viewport ${width}x${height} presentation surface: ` +
      `${JSON.stringify(receipt)}`
    );
  }
  return receipt;
}
