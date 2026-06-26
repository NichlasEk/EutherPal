use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const TV_HTML: &str = include_str!("../../web/tv/index.html");
const MOBILE_HTML: &str = include_str!("../../web/mobile/index.html");
const ADMIN_HTML: &str = include_str!("../../web/admin/index.html");
const STYLES_CSS: &str = include_str!("../../web/shared/styles.css");
const APP_JS: &str = include_str!("../../web/shared/app.js");
const DEFAULT_PREPROMPT: &str = include_str!("../../prompts/supergemma.bank.sv.md");
const SETTINGS_PATH: &str = "data/settings.toml";
const DEFAULT_MODEL: &str = "supergemma";

const TOKEN_CHOICES: &[(&str, &str)] = &[
    ("bil", "Bil"),
    ("hatt", "Hatt"),
    ("skepp", "Skepp"),
    ("hund", "Hund"),
    ("sko", "Sko"),
];

const BOARD_SPACES: [&str; 40] = [
    "Gå",
    "Kristinavägen",
    "Allmänning",
    "Kungsgatan",
    "Inkomstskatt",
    "Lindesberg C",
    "Storgatan",
    "Chans",
    "Rådhustorget",
    "Prästgatan",
    "Fängelse",
    "Köpmangatan",
    "Elverket",
    "Bergslagsvägen",
    "Norrtullsgatan",
    "Frövi station",
    "Bondegatan",
    "Allmänning",
    "Kyrkberget",
    "Smedjegatan",
    "Fri parkering",
    "Loppholmarna",
    "Chans",
    "Strandpromenaden",
    "Sjövägen",
    "Storå station",
    "Björkhyttevägen",
    "Vattenverket",
    "Gusselbyvägen",
    "Löpargatan",
    "Gå i fängelse",
    "Hagavägen",
    "Lasarettsgatan",
    "Allmänning",
    "Brotorpsvägen",
    "Fellingsbro station",
    "Chans",
    "Lindesjön",
    "Lyxskatt",
    "Apothic Avenue",
];

#[derive(Clone)]
struct Player {
    name: String,
    cash: i32,
    position: usize,
    token: Option<String>,
}

struct GameState {
    room_code: String,
    phase: Phase,
    players: Vec<Player>,
    selection_order: Vec<usize>,
    selection_cursor: usize,
    current_player_index: usize,
    dice: [u8; 2],
    bank_message: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    TokenSelection,
    Play,
}

type SharedGame = Arc<Mutex<GameState>>;

fn main() -> std::io::Result<()> {
    let bind_addr = env::var("EUTHERPAL_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let listener = TcpListener::bind(&bind_addr)?;
    let game = Arc::new(Mutex::new(GameState::new()));

    println!("EutherPål dev server listening on http://{bind_addr}");
    println!("TV:     http://{bind_addr}/tv");
    println!("Mobile: http://{bind_addr}/mobile");
    println!("Admin:  http://{bind_addr}/admin");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let game = Arc::clone(&game);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, game) {
                        eprintln!("request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream, game: SharedGame) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header == "\r\n" || header.is_empty() {
            break;
        }
        headers.push(header);
    }

    let method = request_line.split_whitespace().next().unwrap_or("GET");
    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = split_target(target);
    let body = read_body(&mut reader, &headers)?;

    let response = match (method, path) {
        ("GET", "/") => redirect("/tv"),
        ("GET", "/health") => json(200, r#"{"status":"ok","service":"eutherpal","ai":"mock"}"#),
        ("GET", "/api/game") | ("GET", "/api/game/mock") => {
            let game = game.lock().expect("game state lock");
            json(200, &game.to_json())
        }
        ("GET", "/api/settings") => json(200, &load_settings().to_json()),
        ("POST", "/api/settings") => {
            let mut settings = load_settings();
            settings.model = form_value(&body, "model").unwrap_or(settings.model);
            settings.preprompt = form_value(&body, "preprompt").unwrap_or(settings.preprompt);
            match save_settings(&settings) {
                Ok(()) => json(200, &settings.to_json()),
                Err(error) => json(
                    500,
                    &format!("{{\"error\":\"{}\"}}", escape_json(&error.to_string())),
                ),
            }
        }
        ("POST", "/api/game/new") => {
            let mut game = game.lock().expect("game state lock");
            *game = GameState::new();
            json(200, &game.to_json())
        }
        ("POST", "/api/game/select-token") => {
            let token = form_value(&body, "token")
                .or_else(|| query_value(query, "token"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            game.select_token(&token);
            json(200, &game.to_json())
        }
        ("POST", "/api/game/roll") => {
            let mut game = game.lock().expect("game state lock");
            game.roll_current_player();
            json(200, &game.to_json())
        }
        ("GET", "/tv") | ("GET", "/tv/") => html(200, TV_HTML),
        ("GET", "/mobile") | ("GET", "/mobile/") => html(200, MOBILE_HTML),
        ("GET", "/admin") | ("GET", "/admin/") => html(200, ADMIN_HTML),
        ("GET", "/assets/styles.css") => asset(200, "text/css; charset=utf-8", STYLES_CSS),
        ("GET", "/assets/app.js") => asset(200, "application/javascript; charset=utf-8", APP_JS),
        _ => html(404, "<h1>404</h1><p>Sidan finns inte.</p>"),
    };

    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

struct Settings {
    model: String,
    preprompt: String,
}

impl Settings {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            preprompt: DEFAULT_PREPROMPT.trim().to_string(),
        }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"model\":\"{}\",\"preprompt\":\"{}\",\"path\":\"{}\"}}",
            escape_json(&self.model),
            escape_json(&self.preprompt),
            SETTINGS_PATH
        )
    }

    fn to_toml(&self) -> String {
        format!(
            "[llm]\nmodel = \"{}\"\n\n[bank]\npreprompt = \"\"\"\n{}\n\"\"\"\n",
            escape_toml_string(&self.model),
            escape_toml_multiline(&self.preprompt)
        )
    }
}

impl GameState {
    fn new() -> Self {
        let players = vec![
            Player::new("Anna"),
            Player::new("Bo"),
            Player::new("Cleo"),
            Player::new("David"),
        ];
        let first = random_index(players.len());
        let mut selection_order = Vec::new();
        for offset in 0..players.len() {
            selection_order.push((first + offset) % players.len());
        }
        let first_name = players[first].name.clone();

        Self {
            room_code: "PAL-001".to_string(),
            phase: Phase::TokenSelection,
            players,
            selection_order,
            selection_cursor: 0,
            current_player_index: first,
            dice: [0, 0],
            bank_message: format!(
                "{first_name} börjar och väljer pjäs först. Välj en av de fem klassiska pjäserna."
            ),
        }
    }

    fn select_token(&mut self, token: &str) {
        if self.phase != Phase::TokenSelection {
            self.bank_message = "Pjäsvalet är redan klart. Nu är spelet igång.".to_string();
            return;
        }

        if !TOKEN_CHOICES.iter().any(|(id, _)| *id == token) {
            self.bank_message = "Välj en av de fem tillgängliga pjäserna.".to_string();
            return;
        }

        if self
            .players
            .iter()
            .any(|player| player.token.as_deref() == Some(token))
        {
            self.bank_message = "Den pjäsen är redan vald. Välj en ledig pjäs.".to_string();
            return;
        }

        let player_index = self.selection_order[self.selection_cursor];
        let player_name = self.players[player_index].name.clone();
        let token_label = token_label(token);
        self.players[player_index].token = Some(token.to_string());
        self.selection_cursor += 1;

        if self.selection_cursor >= self.selection_order.len() {
            self.phase = Phase::Play;
            self.current_player_index = self.selection_order[0];
            self.bank_message = format!(
                "{player_name} valde {token_label}. Alla pjäser är valda. {} börjar spelet.",
                self.players[self.current_player_index].name
            );
        } else {
            let next = self.selection_order[self.selection_cursor];
            self.bank_message = format!(
                "{player_name} valde {token_label}. {} väljer nästa pjäs.",
                self.players[next].name
            );
        }
    }

    fn roll_current_player(&mut self) {
        if self.phase != Phase::Play {
            self.bank_message =
                "Alla spelare måste välja pjäs innan första tärningsslaget.".to_string();
            return;
        }

        self.dice = [random_die(), random_die()];
        let steps = (self.dice[0] + self.dice[1]) as usize;
        let player = &mut self.players[self.current_player_index];
        let old_position = player.position;
        player.position = (player.position + steps) % BOARD_SPACES.len();
        if old_position + steps >= BOARD_SPACES.len() {
            player.cash += 2000;
        }

        let landed = BOARD_SPACES[player.position];
        self.bank_message = format!(
            "{} slog {} + {} och går till {landed}.",
            player.name, self.dice[0], self.dice[1]
        );

        if self.dice[0] != self.dice[1] {
            self.current_player_index = (self.current_player_index + 1) % self.players.len();
        } else {
            self.bank_message
                .push_str(" Dubbel, samma spelare slår igen.");
        }
    }

    fn current_selector_index(&self) -> usize {
        if self.phase == Phase::TokenSelection {
            self.selection_order[self.selection_cursor]
        } else {
            self.current_player_index
        }
    }

    fn to_json(&self) -> String {
        let current = &self.players[self.current_selector_index()].name;
        let players = self
            .players
            .iter()
            .map(|player| {
                format!(
                    "{{\"name\":\"{}\",\"cash\":{},\"position\":{},\"token\":{}}}",
                    escape_json(&player.name),
                    player.cash,
                    player.position,
                    optional_json_string(player.token.as_deref())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let spaces = BOARD_SPACES
            .iter()
            .map(|space| format!("\"{}\"", escape_json(space)))
            .collect::<Vec<_>>()
            .join(",");
        let token_choices = TOKEN_CHOICES
            .iter()
            .map(|(id, label)| {
                let available = !self
                    .players
                    .iter()
                    .any(|player| player.token.as_deref() == Some(*id));
                format!(
                    "{{\"id\":\"{}\",\"label\":\"{}\",\"available\":{}}}",
                    escape_json(id),
                    escape_json(label),
                    available
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "{{\"roomCode\":\"{}\",\"phase\":\"{}\",\"currentPlayer\":\"{}\",\"bankMessage\":\"{}\",\"dice\":[{},{}],\"players\":[{}],\"tokenChoices\":[{}],\"spaces\":[{}]}}",
            escape_json(&self.room_code),
            if self.phase == Phase::TokenSelection {
                "token_selection"
            } else {
                "play"
            },
            escape_json(current),
            escape_json(&self.bank_message),
            self.dice[0],
            self.dice[1],
            players,
            token_choices,
            spaces
        )
    }
}

impl Player {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            cash: 15000,
            position: 0,
            token: None,
        }
    }
}

fn read_body(reader: &mut BufReader<TcpStream>, headers: &[String]) -> std::io::Result<String> {
    let content_length = headers
        .iter()
        .find_map(|header| {
            let lower = header.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);

    let mut body = vec![0; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(String::from_utf8_lossy(&body).to_string())
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| url_decode(value))
}

fn query_value(query: &str, key: &str) -> Option<String> {
    form_value(query, key)
}

fn url_decode(value: &str) -> String {
    let mut bytes = Vec::new();
    let raw = value.as_bytes();
    let mut index = 0;

    while index < raw.len() {
        match raw[index] {
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < raw.len() => {
                if let Ok(hex) = std::str::from_utf8(&raw[index + 1..index + 3]) {
                    if let Ok(byte) = u8::from_str_radix(hex, 16) {
                        bytes.push(byte);
                        index += 3;
                        continue;
                    }
                }
                bytes.push(raw[index]);
                index += 1;
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&bytes).to_string()
}

fn redirect(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn html(status: u16, body: &str) -> String {
    asset(status, "text/html; charset=utf-8", body)
}

fn json(status: u16, body: &str) -> String {
    asset(status, "application/json; charset=utf-8", body)
}

fn asset(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        _ => "OK",
    };

    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    )
}

fn optional_json_string(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape_json(value)),
        None => "null".to_string(),
    }
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn load_settings() -> Settings {
    let Ok(toml) = fs::read_to_string(SETTINGS_PATH) else {
        return Settings::default();
    };

    let mut settings = Settings::default();
    if let Some(model) = toml_string_value(&toml, "model") {
        settings.model = model;
    }
    if let Some(preprompt) = toml_multiline_value(&toml, "preprompt") {
        settings.preprompt = preprompt.trim().to_string();
    }
    settings
}

fn save_settings(settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = Path::new(SETTINGS_PATH).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(SETTINGS_PATH, settings.to_toml())
}

fn toml_string_value(toml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = ");
    toml.lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .and_then(|value| {
            let value = value.trim();
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(|value| value.replace("\\\"", "\"").replace("\\\\", "\\"))
        })
}

fn toml_multiline_value(toml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"\"\"");
    let start = toml.find(&prefix)? + prefix.len();
    let rest = &toml[start..];
    let end = rest.find("\"\"\"")?;
    Some(rest[..end].to_string())
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_toml_multiline(value: &str) -> String {
    value.replace("\"\"\"", "\\\"\\\"\\\"")
}

fn token_label(token: &str) -> &'static str {
    TOKEN_CHOICES
        .iter()
        .find(|(id, _)| *id == token)
        .map(|(_, label)| *label)
        .unwrap_or("pjäsen")
}

fn random_index(max: usize) -> usize {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % max
}

fn random_die() -> u8 {
    (random_index(6) + 1) as u8
}
