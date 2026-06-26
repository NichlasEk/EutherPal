package se.eutherpal.mobile;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.DialogInterface;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.Window;
import android.view.WindowManager;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.TextView;

public final class MainActivity extends Activity {
    private static final String PREFS = "eutherpal-mobile";
    private static final String KEY_SERVER_URL = "server_url";

    private WebView webView;
    private TextView statusView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        getWindow().setFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN, WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.rgb(29, 32, 33));

        webView = new WebView(this);
        webView.setFocusable(true);
        webView.setFocusableInTouchMode(true);
        webView.requestFocus();
        webView.setBackgroundColor(Color.rgb(29, 32, 33));

        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setLoadWithOverviewMode(true);
        settings.setUseWideViewPort(true);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);
        disableForceDark(settings);

        webView.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageFinished(WebView view, String url) {
                showStatus("");
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) {
                    showStatus("Kunde inte nå EutherPål-servern. Bakåt laddar om. Håll Bakåt för serveradress.");
                }
            }
        });

        statusView = new TextView(this);
        statusView.setTextColor(Color.WHITE);
        statusView.setBackgroundColor(Color.argb(220, 31, 22, 9));
        statusView.setTextSize(16);
        statusView.setGravity(Gravity.CENTER);
        statusView.setPadding(24, 16, 24, 16);
        statusView.setVisibility(View.GONE);

        root.addView(webView, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT));
        FrameLayout.LayoutParams statusParams = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM);
        root.addView(statusView, statusParams);
        setContentView(root);

        webView.loadUrl(serverUrl());
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (event.getAction() == KeyEvent.ACTION_UP) {
            int code = event.getKeyCode();
            if (code == KeyEvent.KEYCODE_BACK) {
                if (event.getEventTime() - event.getDownTime() > 900) {
                    showServerDialog();
                    return true;
                }
                if (webView.canGoBack()) {
                    webView.goBack();
                } else {
                    webView.reload();
                    showStatus("Laddar om EutherPål...");
                }
                return true;
            }
            if (code == KeyEvent.KEYCODE_MENU || code == KeyEvent.KEYCODE_SETTINGS) {
                showServerDialog();
                return true;
            }
        }
        return super.dispatchKeyEvent(event);
    }

    private String serverUrl() {
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        return prefs.getString(KEY_SERVER_URL, getString(R.string.default_server_url));
    }

    private void saveServerUrl(String value) {
        String normalized = value.trim();
        if (!normalized.endsWith("/mobile")) {
            Uri uri = Uri.parse(normalized);
            if (uri.getPath() == null || uri.getPath().isEmpty() || "/".equals(uri.getPath())) {
                normalized = normalized.replaceAll("/+$", "") + "/mobile";
            }
        }
        getSharedPreferences(PREFS, MODE_PRIVATE).edit().putString(KEY_SERVER_URL, normalized).apply();
    }

    private void showServerDialog() {
        final EditText input = new EditText(this);
        input.setSingleLine(true);
        input.setText(serverUrl());
        input.setSelectAllOnFocus(true);
        input.setTextColor(Color.BLACK);

        new AlertDialog.Builder(this)
                .setTitle("EutherPål server")
                .setMessage("Ange mobil-URL eller serverbas. Exempel: http://192.168.32.186:8793/mobile")
                .setView(input)
                .setPositiveButton("Spara", new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        saveServerUrl(input.getText().toString());
                        webView.loadUrl(serverUrl());
                    }
                })
                .setNegativeButton("Avbryt", null)
                .setNeutralButton("Standard", new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        saveServerUrl(getString(R.string.default_server_url));
                        webView.loadUrl(serverUrl());
                    }
                })
                .show();
    }

    private void showStatus(String message) {
        if (message == null || message.isEmpty()) {
            statusView.setVisibility(View.GONE);
        } else {
            statusView.setText(message);
            statusView.setVisibility(View.VISIBLE);
        }
    }

    private void disableForceDark(WebSettings settings) {
        try {
            WebSettings.class.getMethod("setForceDark", int.class).invoke(settings, 0);
        } catch (Exception ignored) {
            // Older WebView versions do not expose force-dark controls.
        }
    }
}
