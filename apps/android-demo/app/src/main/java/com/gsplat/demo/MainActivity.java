package com.gsplat.demo;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;

import java.io.FileOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;

public final class MainActivity extends Activity {
  private static final String MINIMAL_PLY =
      "ply\n"
          + "format ascii 1.0\n"
          + "element vertex 1\n"
          + "property float x\n"
          + "property float y\n"
          + "property float z\n"
          + "property float opacity\n"
          + "property float scale_0\n"
          + "property float scale_1\n"
          + "property float scale_2\n"
          + "property float rot_0\n"
          + "property float rot_1\n"
          + "property float rot_2\n"
          + "property float rot_3\n"
          + "property float f_dc_0\n"
          + "property float f_dc_1\n"
          + "property float f_dc_2\n"
          + "end_header\n"
          + "0.0 0.0 0.5 0.9 1.0 1.0 1.0 1.0 0.0 0.0 0.0 0.9 0.2 0.1\n";

  @Override
  protected void onCreate(Bundle savedInstanceState) {
    super.onCreate(savedInstanceState);

    String datasetPath = getFilesDir().getAbsolutePath() + "/minimal_ascii.ply";
    writeDataset(datasetPath);
    int major = NativeBridge.versionMajor();
    int minor = NativeBridge.versionMinor();
    int rc = NativeBridge.runFfiSmoke(datasetPath);

    TextView text = new TextView(this);
    text.setText(
        "gsplat android demo\n"
            + "abi=" + major + "." + minor + "\n"
            + "ffi_smoke_rc=" + rc + "\n"
            + "dataset=" + datasetPath);
    setContentView(text);
  }

  private void writeDataset(String datasetPath) {
    try (FileOutputStream out = new FileOutputStream(datasetPath)) {
      out.write(MINIMAL_PLY.getBytes(StandardCharsets.UTF_8));
    } catch (IOException ignored) {
      // Failure is reflected by ffi_smoke_rc once load attempts happen.
    }
  }
}
