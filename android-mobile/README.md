# EutherPål Mobile APK

Minimal Android WebView-wrapper för telefonerna. Appen öppnar spelarpanelen på:

`http://192.168.32.186:8793/mobile`

Mobilvyn har två sidor: spelare och admin. Svep vänster för admin och höger för spelare, eller tryck på flikarna högst upp.

## Bygg

```sh
android-mobile/build-apk.sh
```

APK:n skrivs till:

`android-mobile/dist/eutherpal-mobile.apk`

## Installera

```sh
adb install -r android-mobile/dist/eutherpal-mobile.apk
adb shell monkey -p se.eutherpal.mobile 1
```

Håll Bakåt intryckt, eller använd Menu/Settings om telefonen har den knappen, för att ändra server-URL. Om du skriver en serverbas utan sökväg lägger appen automatiskt till `/mobile`.
