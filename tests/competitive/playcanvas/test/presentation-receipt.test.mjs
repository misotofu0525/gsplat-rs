import assert from 'node:assert/strict';
import test from 'node:test';
import {
  assertBrowserPresentationState,
  captureBrowserPresentationState,
  PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX,
  shouldObserveMeasuredPresentation,
  VISUAL_VIEWPORT_QUANTIZATION_TOLERANCE_CSS_PX
} from '../public/presentation-receipt.js';

test('throughput disables per-frame presentation observers while controls retain them', () => {
  assert.equal(shouldObserveMeasuredPresentation(false), false);
  assert.equal(shouldObserveMeasuredPresentation(true), true);
  assert.equal(shouldObserveMeasuredPresentation(undefined), false);
});

function fixture(overrides = {}) {
  let canvas;
  const documentObject = {
    visibilityState: 'visible',
    hidden: false,
    hasFocus: () => true,
    elementFromPoint: () => canvas,
    ...overrides.document
  };
  const windowObject = {
    devicePixelRatio: 1,
    innerWidth: 2412,
    innerHeight: 1080,
    visualViewport: {
      width: 2412,
      height: 1080,
      scale: 1,
      offsetLeft: 0,
      offsetTop: 0,
      ...overrides.visualViewport
    },
    ...overrides.window
  };
  canvas = {
    width: 2412,
    height: 1080,
    getBoundingClientRect: () => ({
      left: 0,
      top: 0,
      width: 2412,
      height: 1080,
      ...overrides.rect
    }),
    ...overrides.canvas
  };
  return { documentObject, windowObject, canvas };
}

test('foreground browser receipt proves focus, viewport, DPR, CSS, and backing dimensions', () => {
  const receipt = captureBrowserPresentationState({
    ...fixture(),
    expectedWidth: 2412,
    expectedHeight: 1080,
    phase: 'pre_measurement',
    now: () => 12.5
  });
  assert.equal(receipt.visibility_state, 'visible');
  assert.equal(receipt.has_focus, true);
  assert.equal(receipt.visual_viewport.scale, 1);
  assert.equal(receipt.canvas.backing_width, 2412);
  assert.equal(receipt.canvas_unobscured.all_samples_hit_canvas, true);
  assert.equal(receipt.canvas_unobscured.inset_css_px, 1);
  assert.equal(receipt.canvas_unobscured.samples.length, 5);
  assert.equal(receipt.canvas_unobscured.samples[0].hit.is_canvas, true);
  assert.equal(receipt.canvas_half_pixel_edge_diagnostic.inset_css_px, 0.5);
  assert.equal(receipt.visual_viewport.dimension_tolerance_css_px, 0.5);
  assert.equal(receipt.physical_pixel_mapping.tolerance_px, 2);
  assert.deepEqual(receipt.safe_area_insets, { observed: false });
  assert.equal(assertBrowserPresentationState(receipt), receipt);
});

test('obscured receipt identifies the exact sample and element stack', () => {
  const overlay = {
    tagName: 'DIV',
    id: 'browser-overlay-proxy',
    className: 'overlay',
    parentElement: null,
    contains: () => false
  };
  const state = fixture({
    document: {
      elementFromPoint: (x, y) => x <= 1 && y <= 1 ? overlay : state.canvas,
      elementsFromPoint: (x, y) => x <= 1 && y <= 1
        ? [overlay, state.canvas]
        : [state.canvas]
    }
  });
  let receipt;
  assert.throws(
    () => {
      receipt = captureBrowserPresentationState({
        ...state,
        expectedWidth: 2412,
        expectedHeight: 1080,
        phase: 'diagnostic',
        now: () => 1
      });
    },
    (error) => {
      const serialized = error.message;
      assert.match(serialized, /browser-overlay-proxy/);
      assert.match(serialized, /top_left/);
      return true;
    }
  );
  assert.equal(receipt, undefined);
});

test('Chromium visual-viewport quantization is bounded without weakening exact backing pixels', () => {
  const observedDeviceWidth = 2412.49;
  assert.ok(
    Math.abs(observedDeviceWidth - 2412) < VISUAL_VIEWPORT_QUANTIZATION_TOLERANCE_CSS_PX
  );
  const receipt = captureBrowserPresentationState({
    ...fixture({
      visualViewport: { width: observedDeviceWidth },
      rect: { width: observedDeviceWidth }
    }),
    expectedWidth: 2412,
    expectedHeight: 1080,
    phase: 'webview_quantization',
    now: () => 1
  });
  assert.equal(receipt.inner_width, 2412);
  assert.equal(receipt.canvas.backing_width, 2412);
  assert.equal(receipt.canvas.css_width, observedDeviceWidth);

  assert.throws(
    () => captureBrowserPresentationState({
      ...fixture({
        visualViewport: { width: 2412.501 },
        rect: { width: 2412.501 }
      }),
      expectedWidth: 2412,
      expectedHeight: 1080,
      phase: 'excessive_quantization',
      now: () => 1
    }),
    /not a focused, visible/
  );
});

test('native Android DPR maps a full CSS viewport onto the exact backing surface', () => {
  const dpr = 2.625;
  const cssWidth = 919;
  const cssHeight = 411;
  const receipt = captureBrowserPresentationState({
    ...fixture({
      window: { devicePixelRatio: dpr, innerWidth: cssWidth, innerHeight: cssHeight },
      visualViewport: { width: cssWidth, height: cssHeight },
      rect: { width: cssWidth, height: cssHeight }
    }),
    expectedWidth: 2412,
    expectedHeight: 1080,
    phase: 'android_native_density',
    now: () => 1
  });
  assert.equal(receipt.device_pixel_ratio, dpr);
  assert.ok(Math.abs(receipt.physical_pixel_mapping.canvas_css_width_px - 2412) <=
    PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX);
  assert.ok(Math.abs(receipt.physical_pixel_mapping.canvas_css_height_px - 1080) <=
    PHYSICAL_PIXEL_MAPPING_TOLERANCE_PX);
});

test('one-pixel canvas gate accepts the observed half-pixel edge quantization only as diagnostics', () => {
  const state = fixture({
    document: {
      elementFromPoint: (x, y) =>
        (x === 0.5 || y === 0.5 || x === 2411.5 || y === 1079.5) ? null : state.canvas,
      elementsFromPoint: (x, y) =>
        (x === 0.5 || y === 0.5 || x === 2411.5 || y === 1079.5) ? [] : [state.canvas]
    }
  });
  const receipt = captureBrowserPresentationState({
    ...state,
    expectedWidth: 2412,
    expectedHeight: 1080,
    phase: 'edge_quantization',
    now: () => 1
  });
  assert.equal(receipt.canvas_unobscured.all_samples_hit_canvas, true);
  assert.equal(receipt.canvas_half_pixel_edge_diagnostic.all_samples_hit_canvas, false);
});

test('background, unfocused, scaled, clipped, and wrong-backing receipts fail closed', () => {
  for (const state of [
    fixture({ document: { visibilityState: 'hidden', hidden: true } }),
    fixture({ document: { hasFocus: () => false } }),
    fixture({ visualViewport: { scale: 0.5 } }),
    fixture({ rect: { height: 1000 } }),
    fixture({ window: { devicePixelRatio: 2.625 } }),
    fixture({ canvas: { width: 1920 } }),
    fixture({ document: { elementFromPoint: () => ({ id: 'overlay' }) } })
  ]) {
    assert.throws(
      () => captureBrowserPresentationState({
        ...state,
        expectedWidth: 2412,
        expectedHeight: 1080,
        phase: 'test',
        now: () => 1
      }),
      /not a focused, visible/
    );
  }
});
