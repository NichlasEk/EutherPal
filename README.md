# EutherPal

AI-drivet Monopol-liknande bradspel pa svenska, byggt for Android TV som huvudskarm och telefoner som spelarhandkontroller.

## Lokalt testlage

Starta servern:

```sh
cargo run --manifest-path server/Cargo.toml
```

Oppna sedan:

- TV: <http://127.0.0.1:8787/tv>
- Mobil: <http://127.0.0.1:8787/mobile>
- Admin: <http://127.0.0.1:8787/admin>
- Health: <http://127.0.0.1:8787/health>

For LAN-test:

```sh
EUTHERPAL_BIND=0.0.0.0:8787 cargo run --manifest-path server/Cargo.toml
```

## Secrets

Riktiga losenord, passfraser, SSH-nycklar, sessionsnycklar och tunneluppgifter ska inte commitas. Example-config innehaller bara dummy-varden.
