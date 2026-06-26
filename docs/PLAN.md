# EutherPal plan

EutherPal ar ett AI-drivet Monopol-liknande bradspel pa svenska. Forsta versionen ska ge en klassisk spelplansupplevelse pa Android TV, med en AI-bank/spelledare som styr spelets flode och spelare som anvander sina telefoner for beslut, fragor och bankarenden.

Namnet i GitHub och kodbasen ar `EutherPal`, medan spelet kan visas for anvandare som `EutherPal` eller `EutherPal - AI Monopol`. Undvik svenska specialtecken i paketnamn, repo, URL:er och tekniska identifierare.

## Produktmal

- Klassisk Monopol-kansla: fyrkantig spelplan, gator, stationer, verk, chans/allmanning, fangelse och fri parkering.
- Android TV som huvudskarm: spelplanen visas pa TV:n och styrs med Android TV-fjarrkontroll.
- Mobilen som spelarens kontrollpanel: varje spelare anvander telefonen for att svara pa fragor, kopa fastigheter, bygga hus/hotell, forhandla och prata med banken.
- AI-bank/spelledare: en lokal LLM agerar bank, spelledare och regelhjalp.
- Svenska som primart sprak i UI, prompts och spelupplevelse.
- Thin client-arkitektur: TV och telefoner ska vara klienter; servern ager spelstatus, regler, sessioner och AI-koppling.
- Admin-installningar for att byta LLM, preprompts, regelprofil och AI-personlighet.

## Rekommenderad teknik

### Server

Rekommendation: Rust med Axum eller Actix Web.

Servern bor aga all auktoritativ spelstatus:

- spelrum och sessionskoder
- spelare och turordning
- tarningsslag och handelser
- fastigheter, agarskap, hyror, hus och hotell
- banktransaktioner
- regelvalidering
- AI-konversationer och admin-installningar

Rust passar bra eftersom servern blir langlivad, lokal, snabb och enkel att deploya som en binar pa `192.168.32.186`.

### TV-klient

Rekommendation for forsta versionen: HTML/CSS/TypeScript som webbaserad TV-klient, paketerad som Android APK via Tauri/Capacitor eller en minimal Android WebView-wrapper.

Skal:

- samma UI kan koras pa `apothictech.se` under utveckling
- Android TV-fjarrkontroll kan mappas till fokusbaserad navigation
- snabbare iteration an full native Android eller spelmotor
- enklare att sideloada som signerad APK

Tauri kan vara intressant om Android-stodet fungerar stabilt i aktuell toolchain. Om det strular bor fallback vara Capacitor eller en mycket liten Kotlin WebView-app som laddar den lokala/hostade TV-klienten.

### Lokalt testlage pa Linux

Utvecklingen ska kunna koras helt pa den har datorn innan Android TV-APK byggs.

Rekommenderat testlage:

- servern kors inte i Android-appen, utan som vanlig Linux-process
- TV-klienten kor i desktop-browser eller WebView-fonster
- mobilklienten kor i mobil-browser, desktop-browser eller Playwright-test
- LLM-koppling kan mockas i borjan och senare peka mot lokal LLM
- samma WebSocket/API-kontrakt anvands for Linux-test, webb och Android TV

Det betyder att forsta milstolparna kan verifieras utan sideloading. Android TV blir en paketerings- och inputfraga, inte en separat spelimplementation.

Om Tauri valjs kan samma frontend paketeras for Linux-desktop och Android TV, men Android-stodet ska provas tidigt. Om Tauri Android blir for skort ska Linux-testlaget anda finnas kvar via vanlig browser och Android TV byggas med enklare WebView/Capacitor.

### Mobilklient

Rekommendation for forsta versionen: mobilwebb/PWA.

Telefonen gar till en URL, anger rums- eller QR-kod och far spelarens kontrollpanel. Detta undviker appdistribution i borjan och fungerar pa bade Android och iPhone.

Senare kan samma mobilwebb paketeras som APK om det behovs.

### Realtidskommunikation

Rekommendation: WebSocket mellan klienter och server.

Kanaler:

- TV-klient: prenumererar pa hela spelrummets publika state.
- Mobilklient: prenumererar pa spelarens privata state och tillatna actions.
- Admin: prenumererar pa driftstatus, LLM-status och spelrum.
- AI-bank: kommunicerar via servern, inte direkt med klienterna.

## Natverksbild

Maskiner:

- Lokal AI-dator: `192.168.32.88`
- Server: `192.168.32.186`
- Publik webbfront: `apothictech.se`

Tankt flode:

1. TV-klienten ansluter till servern via LAN eller `apothictech.se`.
2. Mobilklienter ansluter till samma spelrum via LAN eller `apothictech.se`.
3. Servern pa `192.168.32.186` haller spelstatus och validerar regler.
4. Lokal AI-dator pa `192.168.32.88` kor LLM lokalt.
5. AI-datorn exponerar LLM-tjansten till servern via reverse SSH tunnel.
6. Servern anropar LLM via tunnel och skickar svar tillbaka till TV/mobilklienterna.

Viktigt: losenord, passfraser, SSH-nycklar, tunnelkommandon med hemligheter och sudo-losenord ska inte laggas i repo. Undvik aven plaintext-hemligheter i vanliga configfiler. Forsta driftversionen bor hellre anvanda SSH-agent, interaktiv upplasning vid start, systemd credentials eller annan lokal credential-store dar hemligheten bara finns i minnet nar tjansten kor.

## Android TV-upplevelse

TV:n ar den gemensamma spelplanen.

Fjarrkontrollen ska klara:

- navigera mellan menyval
- starta/fortsatta spel
- visa spelare
- visa fastighetskort
- oppna enklare admin-/debugvy vid behov
- be krafta om TV-specifika steg

TV:n ska inte vara den primara platsen for textinmatning. Alla langre val och konversationer sker pa telefonen.

Visuellt for forsta versionen:

- klassisk kvadratisk spelplan
- tydliga svenska gatunamn
- spelpjaser pa bradet
- aktuell tur och tarningsresultat
- diskret logg fran banken/spelledaren
- tydlig QR-kod eller rumskod for telefonanslutning

## Mobilupplevelse

Telefonen ar spelarens hand och dialog med banken.

Mobilen ska kunna:

- ga med i spelrum
- valja/namnge spelare
- se pengar, fastigheter och status
- kasta tarning nar det ar spelarens tur
- svara pa AI-bankens fragor
- kopa eller avsta fastighet
- bjuda i auktion
- bygga hus/hotell
- salja eller belana
- forhandla med andra spelare
- chatta/prata med banken

Forsta versionen kan borja med text. Rost kan laggas till senare.

## AI-bank och spelledare

AI:n ska inte vara ensam auktoritet for spelregler. Den ska agera sprakligt lager och spelledare, medan servern validerar regelbeslut deterministiskt.

Ansvar for AI:

- forklara vems tur det ar
- stalla fragor till spelaren
- sammanfatta handelser pa svenska
- ge regelhjalp
- foresla tillatna actions
- skapa personlighet och tempo i spelet

Ansvar for servern:

- avgora vilka actions som ar lagliga
- rakna pengar
- flytta pjaser
- rakna hyra
- hantera hus/hotell
- hantera konkurs
- lagra spelstatus

Admin ska kunna andra:

- LLM-endpoint
- modellnamn/profil
- systemprompt/preprompt
- spelledarpersonlighet
- strikt/lekfull regelton
- loggniva

## Sakerhet och drift

Prioritet fran start:

- inga hemligheter i git
- inga plaintext-losenord i repo, `.env` eller vanliga configfiler
- `.env.example` eller example TOML far bara innehalla namn pa variabler och dummy-varden
- stod for interaktiv secret-inmatning vid start for hemmalage
- stod for SSH-agent/systemd credentials for unattended drift
- autentiserad adminvy
- LAN-lage och publikt lage separerade i config
- reverse SSH tunnel som systemd-service
- servern ska kunna starta utan AI och visa tydligt "AI offline"
- loggar ska inte skriva ut hemligheter

Foreslagna config-varden:

- `PUBLIC_BASE_URL`
- `LAN_BASE_URL`
- `DATABASE_URL`
- `LLM_BASE_URL`
- `LLM_MODEL`
- `LLM_SYSTEM_PROMPT_PATH`
- `ADMIN_PASSWORD_HASH`
- `SESSION_SECRET`

Hemliga varden som `ADMIN_PASSWORD_HASH`, `SESSION_SECRET`, SSH-passfraser och tunneluppgifter ska hanteras som runtime secrets. I strikt hemmalage kan de matas in manuellt vid start och hallas i processminne. For automatisk omstart bor de ligga i en riktig credential-losning, inte i projektfiler.

## Databas

Forsta versionen kan anvanda SQLite pa servern.

Tabeller/objekt:

- users/admins
- games
- players
- board_spaces
- properties
- ownership
- transactions
- turns
- events
- ai_messages
- settings

SQLite racker for hemmabruk och ar enkel att backa upp. Om spelet senare ska ha manga samtidiga publika rum kan PostgreSQL laggas till.

## Regler och innehall

Eftersom Monopol ar ett skyddat varumarke och har upphovsrattsskyddade detaljer bor projektet sikta pa ett "Monopol-liknande" spel med egen spelplan, egna gatunamn och egna korttexter.

Forsta innehallsprofil:

- svenska gatunamn med Euther-tema
- klassisk bradlayout med 40 rutor
- hyresstegar inspirerade av klassisk struktur men egna siffror kan justeras
- egna chans-/allmanningkort
- svenska regler i admin-redigerbara datafiler

## Foreslagen repo-struktur

```text
EutherPal/
  docs/
    PLAN.md
  server/
    src/
    migrations/
  web/
    tv/
    mobile/
    admin/
  android-tv/
    README.md
  prompts/
    bank.default.sv.md
  config/
    eutherpal.example.toml
  scripts/
```

## Milstolpar

### Milstolpe 1: Projektgrund

- Skapa repo-struktur.
- Lagg till Rust-server med health endpoint.
- Lagg till enkel web-TV-klient.
- Lagg till enkel mobilwebb.
- Lagg till `.env.example` eller example TOML.
- Verifiera lokalt pa Linux och LAN.
- Lagg till testlage dar TV-klient och mobilklient kan koras pa samma dator.

### Milstolpe 2: Spelrum och spelplan

- Skapa spelrum.
- Visa klassisk 40-rutors spelplan pa TV.
- Lata telefoner joina med kod.
- Visa spelare pa bradet.
- Synka state via WebSocket.

### Milstolpe 3: Grundregler

- Turordning.
- Tarningsslag.
- Flytt runt bradet.
- Kop av fastighet.
- Hyra.
- Pengar.
- Enkel handelselogg.

### Milstolpe 4: AI-bank

- Lokal LLM-endpoint via tunnel.
- Serveradapter for LLM.
- Svensk systemprompt for bank/spelledare.
- AI-meddelanden i TV-logg och mobil.
- Fallback nar AI ar offline.

### Milstolpe 5: Android TV APK

- Fjarrkontrollsanpassad navigation.
- Bygg signerad sideload-APK.
- Testa pa Android TV.
- Dokumentera installation och uppdatering.

### Milstolpe 6: Admin

- Admininloggning.
- Byt LLM-endpoint/modell.
- Redigera promptprofil.
- Visa tunnel/AI-status.
- Visa aktiva spelrum.

## Oppna designbeslut

- Tauri Android, Capacitor eller Kotlin WebView for TV-APK.
- Exakt LLM-runtime pa `192.168.32.88`.
- Om `apothictech.se` ska proxy:a WebSocket direkt till spelservern eller via separat reverse proxy path.
- Om speldata ska vara hardkodad i Rust, TOML/JSON-filer eller databasstyrd fran start.
- Hur mycket roststod som ska finnas i forsta versionen.

## Rekommenderad start

Borja med server + web-TV + mobilwebb. Hall Android TV-APK som paketering av samma TV-klient nar spelplan och fjarrkontroll fungerar i browsern.

Den viktigaste tekniska principen ar att servern ager reglerna och spelets sanning. AI:n ska ge personlighet, dialog och hjalp, men servern ska alltid kunna verifiera och korrigera spelhandelser.
