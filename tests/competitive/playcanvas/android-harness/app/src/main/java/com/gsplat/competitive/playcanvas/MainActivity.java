package com.gsplat.competitive.playcanvas;

import android.app.Activity;
import android.graphics.Color;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.util.Log;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;

/**
 * A presentation-only host for the pinned PlayCanvas qualification page.
 *
 * <p>The activity deliberately contains no renderer, asset conversion, resolution scaling, or
 * benchmark logic. It only gives the existing browser harness an edge-to-edge hardware-accelerated
 * WebView surface and exposes that surface through WebView CDP.</p>
 */
public final class MainActivity extends Activity {
    public static final String EXTRA_URL = "qualification_url";
    private static final String TAG = "PlayCanvasHarness";

    private WebView webView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        configureWindow();
        WebView.setWebContentsDebuggingEnabled(true);

        String url = getIntent().getStringExtra(EXTRA_URL);
        if (!isAllowedQualificationUrl(url)) {
            showFatal("Missing or invalid loopback qualification_url intent extra");
            return;
        }

        webView = new WebView(this);
        webView.setBackgroundColor(Color.BLACK);
        webView.setLayerType(View.LAYER_TYPE_HARDWARE, null);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowContentAccess(false);
        settings.setAllowFileAccess(false);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(false);
        webView.setWebViewClient(new QualificationWebViewClient());
        setContentView(webView, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
        webView.post(this::applyImmersiveMode);
        webView.loadUrl(url);
        Log.i(TAG, "loading pinned qualification URL " + url);
    }

    @Override
    protected void onResume() {
        super.onResume();
        applyImmersiveMode();
        if (webView != null) {
            webView.onResume();
        }
    }

    @Override
    protected void onPause() {
        if (webView != null) {
            webView.onPause();
        }
        super.onPause();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            applyImmersiveMode();
        }
    }

    @Override
    protected void onDestroy() {
        if (webView != null) {
            webView.stopLoading();
            webView.destroy();
            webView = null;
        }
        super.onDestroy();
    }

    private void configureWindow() {
        Window window = getWindow();
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WindowManager.LayoutParams attributes = window.getAttributes();
            attributes.layoutInDisplayCutoutMode =
                    Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
                            ? WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
                            : WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            window.setAttributes(attributes);
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
        }
    }

    @SuppressWarnings("deprecation")
    private void applyImmersiveMode() {
        Window window = getWindow();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                controller.hide(WindowInsets.Type.systemBars());
            }
        } else {
            window.getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                            | View.SYSTEM_UI_FLAG_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
        }
    }

    private static boolean isAllowedQualificationUrl(String url) {
        if (url == null) {
            return false;
        }
        Uri uri = Uri.parse(url);
        String host = uri.getHost();
        return "http".equals(uri.getScheme())
                && ("127.0.0.1".equals(host) || "localhost".equals(host))
                && uri.getPort() >= 1
                && uri.getUserInfo() == null;
    }

    private void showFatal(String message) {
        Log.e(TAG, message);
        TextView text = new TextView(this);
        text.setText(message);
        text.setTextColor(Color.WHITE);
        text.setBackgroundColor(Color.BLACK);
        text.setTextSize(18);
        setContentView(text);
        text.post(this::applyImmersiveMode);
    }

    private static final class QualificationWebViewClient extends WebViewClient {
        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            return !isAllowedQualificationUrl(request.getUrl().toString());
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            Log.e(TAG, "WebView renderer exited: didCrash=" + detail.didCrash()
                    + " priority=" + detail.rendererPriorityAtExit());
            return false;
        }
    }
}
