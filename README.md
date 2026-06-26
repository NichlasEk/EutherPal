# EutherPål

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

## Settings och regler

Adminvyn har en settings-meny for modellval och bankens preprompt.

- Defaultmodell: `supergemma`
- Runtime-settings: `data/settings.toml`
- Example-settings: `config/settings.example.toml`
- Regelprofil: `rules/monopoly.sv.toml`
- Bräddata: `rules/board.lindesberg.toml`

`data/` ar git-ignorerad. Det gor att admin eller AI kan spara lokala settings utan att hemligheter eller experiment hamnar i repo av misstag.
