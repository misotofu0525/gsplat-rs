export const RASTER_DIAGNOSTIC_ID = 'raster_diagnostic_v1';

const PROPERTIES = [
  'x', 'y', 'z', 'opacity',
  'scale_0', 'scale_1', 'scale_2',
  'rot_0', 'rot_1', 'rot_2', 'rot_3',
  'f_dc_0', 'f_dc_1', 'f_dc_2'
];

const VERTICES = [
  [-0.4, 0.0, 0.0, 4.0, -2.5, -2.5, -2.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
  [0.4, 0.2, 0.5, 3.0, -2.6, -2.3, -2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
  [0.0, -0.35, 0.8, 2.5, -2.4, -2.7, -2.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
];

export function createRasterDiagnosticPly() {
  const header = [
    'ply',
    'format binary_little_endian 1.0',
    'comment deterministic uncapped raster diagnostic v1',
    `element vertex ${VERTICES.length}`,
    ...PROPERTIES.map((name) => `property float ${name}`),
    'end_header',
    ''
  ].join('\n');
  const headerBytes = new TextEncoder().encode(header);
  const bytes = new Uint8Array(headerBytes.length + VERTICES.length * PROPERTIES.length * 4);
  bytes.set(headerBytes);
  const view = new DataView(bytes.buffer);
  let offset = headerBytes.length;
  for (const vertex of VERTICES) {
    for (const value of vertex) {
      view.setFloat32(offset, value, true);
      offset += 4;
    }
  }
  return bytes;
}
