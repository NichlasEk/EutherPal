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
import java.io.IOException;
import java.net.HttpURLConnection;
import java.net.URL;

public final class MainActivity extends Activity {
    private static final String PREFS = "eutherpal-mobile";
    private static final String KEY_SERVER_URL = "server_url";
    private static final String AUTO_SERVER_URL = "auto";

    private WebView webView;
    private TextView statusView;
    private boolean triedPublicFallback = false;

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
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        disableForceDark(settings);

        webView.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageFinished(WebView view, String url) {
                showStatus("");
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) {
                    if (AUTO_SERVER_URL.equals(serverUrl()) && !triedPublicFallback) {
                        triedPublicFallback = true;
                        showStatus("Provar apothictech.se...");
                        view.loadUrl(cacheBustedUrl(getString(R.string.public_server_url)));
                        return;
                    }
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

        loadServerUrl();
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
                    loadServerUrl();
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
        return prefs.getString(KEY_SERVER_URL, AUTO_SERVER_URL);
    }

    private void loadServerUrl() {
        webView.clearCache(true);
        triedPublicFallback = false;
        String url = serverUrl();
        if (AUTO_SERVER_URL.equals(url)) {
            showStatus("Letar EutherPål på LAN...");
            loadAutoServerUrl();
            return;
        }
        webView.loadUrl(cacheBustedUrl(url));
    }

    private void loadAutoServerUrl() {
        new Thread(new Runnable() {
            @Override
            public void run() {
                final String selected = probeUrl(getString(R.string.lan_server_url))
                        ? getString(R.string.lan_server_url)
                        : getString(R.string.public_server_url);
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        showStatus(selected.contains("apothictech.se") ? "LAN saknas. Ansluter via apothictech.se..." : "Ansluter via LAN...");
                        webView.loadUrl(cacheBustedUrl(selected));
                    }
                });
            }
        }).start();
    }

    private boolean probeUrl(String url) {
        HttpURLConnection connection = null;
        try {
            connection = (HttpURLConnection) new URL(url.replace("/mobile", "/health")).openConnection();
            connection.setConnectTimeout(1200);
            connection.setReadTimeout(1200);
            connection.setRequestMethod("GET");
            int status = connection.getResponseCode();
            return status >= 200 && status < 500;
        } catch (IOException error) {
            return false;
        } finally {
            if (connection != null) connection.disconnect();
        }
    }

    private String cacheBustedUrl(String url) {
        String separator = url.contains("?") ? "&" : "?";
        return url + separator + "_ep=" + System.currentTimeMillis();
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
        input.setText(AUTO_SERVER_URL.equals(serverUrl()) ? "" : serverUrl());
        input.setSelectAllOnFocus(true);
        input.setTextColor(Color.BLACK);

        new AlertDialog.Builder(this)
                .setTitle("EutherPål server")
                .setMessage("Lämna tomt för auto: LAN först, sedan apothictech.se. Annars ange mobil-URL eller serverbas.")
                .setView(input)
                .setPositiveButton("Spara", new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        String value = input.getText().toString().trim();
                        if (value.isEmpty()) {
                            getSharedPreferences(PREFS, MODE_PRIVATE).edit().putString(KEY_SERVER_URL, AUTO_SERVER_URL).apply();
                        } else {
                            saveServerUrl(value);
                        }
                        loadServerUrl();
                    }
                })
                .setNegativeButton("Avbryt", null)
                .setNeutralButton("Standard", new DialogInterface.OnClickListener() {
                    @Override
                    public void onClick(DialogInterface dialog, int which) {
                        getSharedPreferences(PREFS, MODE_PRIVATE).edit().putString(KEY_SERVER_URL, AUTO_SERVER_URL).apply();
                        loadServerUrl();
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
