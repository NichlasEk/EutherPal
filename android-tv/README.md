# Android TV packaging

Första Android TV-versionen är en minimal native WebView-wrapper. Den är en thin
client och äger inga regler eller spelstatus.

Default-TV-URL:

```text
http://192.168.32.186:8793/tv
```

## Bygg signerad sideload-APK

```sh
android-tv/build-apk.sh
```

Output:

```text
android-tv/dist/eutherpal-tv.apk
```

Scriptet använder Android SDK direkt (`aapt2`, `javac`, `d8`, `zipalign`,
`apksigner`) och skapar en lokal dev-keystore i `android-tv/.keystore/`.
Keystore och APK-filer är git-ignorerade. Default-lösenorden är bara för lokal
devsignering; sätt dessa env vars om du vill använda en egen signerare:

```sh
EUTHERPAL_KEYSTORE_PASS=... EUTHERPAL_KEY_PASS=... android-tv/build-apk.sh
```

## Installera på Android TV

```sh
adb install -r android-tv/dist/eutherpal-tv.apk
adb shell monkey -p se.eutherpal.tv 1
```

## Fjärrkontroll

- D-pad/OK skickas in i WebView.
- Back laddar om TV-vyn.
- Menu/Settings öppnar server-URL-dialogen.
- Långtryck OK öppnar också server-URL-dialogen.
