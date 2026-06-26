# Android TV packaging

Forsta versionen utvecklas som webbaserad TV-klient och testas pa Linux via `cargo run`.

Nar TV-vyn och fjarrkontrollsnavigationen fungerar ska den paketeras som signerad sideload-APK. Primart spar:

1. Prova Tauri Android om toolchain ar stabil.
2. Fallback till Capacitor.
3. Sista fallback ar en minimal Kotlin WebView-app som laddar TV-klienten.

APK:n ska vara en thin client. Den ska inte aga regler eller spelstatus.
