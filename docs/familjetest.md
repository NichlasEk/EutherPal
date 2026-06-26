# EutherPål familjetest

## Starta

```sh
cargo run --manifest-path server/Cargo.toml
```

TV-skärm:

```text
http://127.0.0.1:8791/tv
```

Mobiler:

```text
http://127.0.0.1:8791/mobile
```

Admin/bank:

```text
http://127.0.0.1:8791/admin
```

## Spela

1. Öppna TV-vyn på skärmen.
2. Varje spelare öppnar mobilvyn, skriver sitt namn och väljer pjäs när det är deras tur.
3. Mobilen visar alltid egen pjäs, pengar och position.
4. Banken kan svara automatiskt i mobilchatten.
5. Admin kan skriva manuellt som banken i adminvyn.

## Adminverktyg

- `Spara spel` skriver aktuell state till `data/game-state.json`.
- `Ladda spel` läser tillbaka `data/game-state.json`.
- `Demo-seed` skapar ett pågående testspel med ägare, byggnader, inteckning och fängelse.
- `Nollställ` startar nytt testspel.
- Justeringsformuläret kan lägga till/dra pengar och flytta en spelare till ruta `0-39`.

## Om något strular

- Spara spelet innan ni testar riskabla regler.
- Använd adminjustering om någon hamnar fel eller pengar behöver korrigeras.
- Om servern startas om: öppna admin och tryck `Ladda spel`.
- Om mobilens namn verkar fel: skriv rätt namn igen och tryck `Spara`.
