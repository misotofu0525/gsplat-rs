import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const declarations = await readFile(
  new URL("../src/index.d.ts", import.meta.url),
  "utf8",
);

test("published declarations expose calibrated camera input and receipt", () => {
  assert.match(
    declarations,
    /export interface GsplatCameraIntrinsics[\s\S]*focalLengthXOverY\?: number;/,
  );
  assert.match(
    declarations,
    /export interface GsplatCameraReceipt[\s\S]*focalLengthXOverY: number;/,
  );
  assert.match(declarations, /setCamera\(camera: GsplatCamera\): void;/);
  assert.match(declarations, /cameraReceipt\(\): GsplatCameraReceipt;/);
});
