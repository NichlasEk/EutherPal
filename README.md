# EutherPål

AI-drivet Monopol-liknande bradspel pa svenska, byggt for Android TV som huvudskarm och telefoner som spelarhandkontroller.

## Lokalt testlage

Starta servern:

```sh
cargo run --manifest-path server/Cargo.toml
```

Oppna sedan:

- TV: <http://127.0.0.1:8793/tv>
- Mobil: <http://127.0.0.1:8793/mobile>
- Admin: <http://127.0.0.1:8793/admin>
- Health: <http://127.0.0.1:8793/health>

For LAN-test:

```sh
EUTHERPAL_BIND=0.0.0.0:8793 cargo run --manifest-path server/Cargo.toml
```

## Android APK

TV-wrappern oppnar `/tv` i fullscreen WebView:

```sh
android-tv/build-apk.sh
```

Mobil-wrappern oppnar `/mobile` och har swipe mellan spelarlage och admin:

```sh
android-mobile/build-apk.sh
```

Byggena hamnar i `android-tv/dist/eutherpal-tv.apk` och `android-mobile/dist/eutherpal-mobile.apk`. Keystore, build-mappar och APK-filer ar git-ignorerade.

Servern behover inte bygga APK:er. Bygg dem pa denna dator och ladda upp dem till de dist-paths som EutherOxide redan serverar:

```sh
scripts/deploy-apks.sh
```

Scriptet bygger TV och mobil lokalt, laddar upp till `euther-server:/home/nichlas/EutherPal/android-*/dist/` och verifierar download-URL:erna:

- <http://192.168.32.186:8080/downloads/EutherPalTV-release-signed.apk>
- <http://192.168.32.186:8080/downloads/EutherPalMobile-release-signed.apk>

## Secrets

Riktiga losenord, passfraser, SSH-nycklar, sessionsnycklar och tunneluppgifter ska inte commitas. Example-config innehaller bara dummy-varden.

## Settings och regler

Adminvyn har en settings-meny for modellval och bankens preprompt.

- Defaultmodell: `supergemma`
- Runtime-settings: `data/settings.toml`
- Example-settings: `config/settings.example.toml`
- Regelprofil: `rules/monopoly.sv.toml`
- Bräddata: `rules/board.lindesberg.toml`

`data/` ar git-ignorerad. Det gor att admin eller AI kan spara lokala settings utan att hemligheter eller experiment hamnar i repo av misstag.
