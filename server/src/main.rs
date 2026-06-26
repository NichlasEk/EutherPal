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
const BOARD_TOML: &str = include_str!("../../rules/board.lindesberg.toml");
const SETTINGS_PATH: &str = "data/settings.toml";
const DEFAULT_MODEL: &str = "supergemma";

const TOKEN_CHOICES: &[(&str, &str)] = &[
    ("bil", "Bil"),
    ("hatt", "Hatt"),
    ("skepp", "Skepp"),
    ("hund", "Hund"),
    ("sko", "Sko"),
];

#[derive(Clone)]
struct Player {
    name: String,
    cash: i32,
    position: usize,
    token: Option<String>,
}

#[derive(Clone)]
struct BoardSpace {
    index: usize,
    kind: String,
    name: String,
    color: Option<String>,
    price: Option<i32>,
    rent: Option<i32>,
}

struct PendingOffer {
    player_index: usize,
    space_index: usize,
}

struct AuctionState {
    space_index: usize,
    seller_turn_index: usize,
    highest_bid: i32,
    highest_bidder: Option<usize>,
}

struct GameState {
    room_code: String,
    phase: Phase,
    players: Vec<Player>,
    spaces: Vec<BoardSpace>,
    owners: Vec<Option<usize>>,
    selection_order: Vec<usize>,
    selection_cursor: usize,
    current_player_index: usize,
    dice: [u8; 2],
    pending_offer: Option<PendingOffer>,
    auction: Option<AuctionState>,
    bank_message: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    TokenSelection,
    Play,
    Auction,
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
        ("POST", "/api/game/buy") => {
            let mut game = game.lock().expect("game state lock");
            game.buy_pending_property();
            json(200, &game.to_json())
        }
        ("POST", "/api/game/decline") => {
            let mut game = game.lock().expect("game state lock");
            game.decline_pending_property();
            json(200, &game.to_json())
        }
        ("POST", "/api/game/auction/bid") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let amount = form_value(&body, "amount")
                .or_else(|| query_value(query, "amount"))
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let mut game = game.lock().expect("game state lock");
            game.place_auction_bid(&player, amount);
            json(200, &game.to_json())
        }
        ("POST", "/api/game/auction/finish") => {
            let mut game = game.lock().expect("game state lock");
            game.finish_auction();
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
        let spaces = parse_board_spaces(BOARD_TOML);
        let owners = vec![None; spaces.len()];
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
            spaces,
            owners,
            selection_order,
            selection_cursor: 0,
            current_player_index: first,
            dice: [0, 0],
            pending_offer: None,
            auction: None,
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
        if self.pending_offer.is_some() {
            self.bank_message = "Avsluta köpfrågan innan nästa tärningsslag.".to_string();
            return;
        }
        if self.auction.is_some() {
            self.bank_message = "Avsluta auktionen innan nästa tärningsslag.".to_string();
            return;
        }

        self.dice = [random_die(), random_die()];
        let steps = (self.dice[0] + self.dice[1]) as usize;
        let player = &mut self.players[self.current_player_index];
        let old_position = player.position;
        player.position = (player.position + steps) % self.spaces.len();
        if old_position + steps >= self.spaces.len() {
            player.cash += 2000;
        }

        let landed_index = player.position;
        let landed = self.spaces[landed_index].clone();
        self.bank_message = format!(
            "{} slog {} + {} och går till {}.",
            player.name, self.dice[0], self.dice[1], landed.name
        );

        if landed.is_buyable() {
            match self.owners[landed_index] {
                None => {
                    let price = landed.price.unwrap_or(0);
                    self.pending_offer = Some(PendingOffer {
                        player_index: self.current_player_index,
                        space_index: landed_index,
                    });
                    self.bank_message
                        .push_str(&format!(" Vill du köpa {} för {} kr?", landed.name, price));
                    return;
                }
                Some(owner_index) if owner_index != self.current_player_index => {
                    let rent = landed.rent.unwrap_or(0);
                    let payer_name = self.players[self.current_player_index].name.clone();
                    let owner_name = self.players[owner_index].name.clone();
                    self.players[self.current_player_index].cash -= rent;
                    self.players[owner_index].cash += rent;
                    self.bank_message.push_str(&format!(
                        " {} äger rutan. {payer_name} betalar {} kr i hyra till {owner_name}.",
                        owner_name, rent
                    ));
                }
                Some(_) => {
                    self.bank_message.push_str(" Du äger redan den här rutan.");
                }
            }
        }

        self.finish_turn_after_roll();
    }

    fn buy_pending_property(&mut self) {
        let Some(offer) = self.pending_offer.take() else {
            self.bank_message = "Det finns ingen fastighet att köpa just nu.".to_string();
            return;
        };
        if offer.player_index != self.current_player_index {
            self.bank_message = "Det är inte rätt spelare för den här köpfrågan.".to_string();
            self.pending_offer = Some(offer);
            return;
        }

        let space = self.spaces[offer.space_index].clone();
        let price = space.price.unwrap_or(0);
        if self.players[offer.player_index].cash < price {
            self.bank_message = format!(
                "{} har inte råd att köpa {} för {} kr.",
                self.players[offer.player_index].name, space.name, price
            );
            self.finish_turn_after_roll();
            return;
        }

        self.players[offer.player_index].cash -= price;
        self.owners[offer.space_index] = Some(offer.player_index);
        self.bank_message = format!(
            "{} köper {} för {} kr.",
            self.players[offer.player_index].name, space.name, price
        );
        self.finish_turn_after_roll();
    }

    fn decline_pending_property(&mut self) {
        let Some(offer) = self.pending_offer.take() else {
            self.bank_message = "Det finns ingen köpfråga att avstå.".to_string();
            return;
        };
        let space = self.spaces[offer.space_index].clone();
        self.phase = Phase::Auction;
        self.auction = Some(AuctionState {
            space_index: offer.space_index,
            seller_turn_index: offer.player_index,
            highest_bid: 0,
            highest_bidder: None,
        });
        self.bank_message = format!(
            "{} avstår från att köpa {}. Auktionen startar på 0 kr.",
            self.players[offer.player_index].name, space.name
        );
    }

    fn place_auction_bid(&mut self, player_name: &str, amount: i32) {
        let Some(auction) = &mut self.auction else {
            self.bank_message = "Det finns ingen aktiv auktion.".to_string();
            return;
        };
        let Some(player_index) = self
            .players
            .iter()
            .position(|player| player.name == player_name)
        else {
            self.bank_message = "Okänd spelare kan inte lägga bud.".to_string();
            return;
        };
        if amount <= auction.highest_bid {
            self.bank_message = format!("Budet måste vara högre än {} kr.", auction.highest_bid);
            return;
        }
        if self.players[player_index].cash < amount {
            self.bank_message = format!(
                "{} har inte råd att bjuda {} kr.",
                self.players[player_index].name, amount
            );
            return;
        }

        auction.highest_bid = amount;
        auction.highest_bidder = Some(player_index);
        let space_name = self.spaces[auction.space_index].name.clone();
        self.bank_message = format!(
            "{} leder auktionen på {} med {} kr.",
            self.players[player_index].name, space_name, amount
        );
    }

    fn finish_auction(&mut self) {
        let Some(auction) = self.auction.take() else {
            self.bank_message = "Det finns ingen aktiv auktion att avsluta.".to_string();
            return;
        };

        let space = self.spaces[auction.space_index].clone();
        if let Some(winner) = auction.highest_bidder {
            self.players[winner].cash -= auction.highest_bid;
            self.owners[auction.space_index] = Some(winner);
            self.bank_message = format!(
                "{} vinner auktionen på {} för {} kr.",
                self.players[winner].name, space.name, auction.highest_bid
            );
        } else {
            self.bank_message = format!(
                "Ingen lade bud på {}. Rutan är fortfarande ledig.",
                space.name
            );
        }

        self.phase = Phase::Play;
        self.finish_turn_after_roll();
    }

    fn finish_turn_after_roll(&mut self) {
        if self.dice[0] == self.dice[1] {
            self.bank_message
                .push_str(" Dubbel, samma spelare slår igen.");
        } else {
            self.current_player_index = (self.current_player_index + 1) % self.players.len();
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
        let spaces = self
            .spaces
            .iter()
            .map(|space| {
                let owner = self.owners[space.index]
                    .map(|owner| format!("\"{}\"", escape_json(&self.players[owner].name)))
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    "{{\"index\":{},\"kind\":\"{}\",\"name\":\"{}\",\"color\":{},\"price\":{},\"rent\":{},\"owner\":{}}}",
                    space.index,
                    escape_json(&space.kind),
                    escape_json(&space.name),
                    optional_json_string(space.color.as_deref()),
                    optional_json_number(space.price),
                    optional_json_number(space.rent),
                    owner
                )
            })
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
            "{{\"roomCode\":\"{}\",\"phase\":\"{}\",\"currentPlayer\":\"{}\",\"bankMessage\":\"{}\",\"dice\":[{},{}],\"pendingOffer\":{},\"auction\":{},\"players\":[{}],\"tokenChoices\":[{}],\"spaces\":[{}]}}",
            escape_json(&self.room_code),
            self.phase.as_str(),
            escape_json(current),
            escape_json(&self.bank_message),
            self.dice[0],
            self.dice[1],
            self.pending_offer_json(),
            self.auction_json(),
            players,
            token_choices,
            spaces
        )
    }

    fn pending_offer_json(&self) -> String {
        let Some(offer) = &self.pending_offer else {
            return "null".to_string();
        };
        let space = &self.spaces[offer.space_index];
        format!(
            "{{\"player\":\"{}\",\"spaceIndex\":{},\"spaceName\":\"{}\",\"price\":{}}}",
            escape_json(&self.players[offer.player_index].name),
            offer.space_index,
            escape_json(&space.name),
            space.price.unwrap_or(0)
        )
    }

    fn auction_json(&self) -> String {
        let Some(auction) = &self.auction else {
            return "null".to_string();
        };
        let highest_bidder = auction
            .highest_bidder
            .map(|index| format!("\"{}\"", escape_json(&self.players[index].name)))
            .unwrap_or_else(|| "null".to_string());
        let next_bid = if auction.highest_bid == 0 {
            100
        } else {
            auction.highest_bid + 100
        };
        format!(
            "{{\"spaceIndex\":{},\"spaceName\":\"{}\",\"highestBid\":{},\"highestBidder\":{},\"nextBid\":{},\"seller\":\"{}\"}}",
            auction.space_index,
            escape_json(&self.spaces[auction.space_index].name),
            auction.highest_bid,
            highest_bidder,
            next_bid,
            escape_json(&self.players[auction.seller_turn_index].name)
        )
    }
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::TokenSelection => "token_selection",
            Phase::Play => "play",
            Phase::Auction => "auction",
        }
    }
}

impl BoardSpace {
    fn is_buyable(&self) -> bool {
        matches!(self.kind.as_str(), "property" | "station" | "utility")
            && self.price.is_some()
            && self.rent.is_some()
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

fn optional_json_number(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn parse_board_spaces(toml: &str) -> Vec<BoardSpace> {
    let mut spaces = Vec::new();
    let mut current = Vec::new();

    for line in toml.lines() {
        if line.trim() == "[[spaces]]" {
            if !current.is_empty() {
                spaces.push(parse_space_block(&current));
                current.clear();
            }
        } else if !line.trim().is_empty() {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        spaces.push(parse_space_block(&current));
    }

    spaces.sort_by_key(|space| space.index);
    spaces
}

fn parse_space_block(lines: &[String]) -> BoardSpace {
    let mut index = 0;
    let mut kind = String::new();
    let mut name = String::new();
    let mut color = None;
    let mut price = None;
    let mut rent = None;

    for line in lines {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "index" => index = value.parse().unwrap_or(0),
            "kind" => kind = trim_toml_quotes(value).to_string(),
            "name" => name = trim_toml_quotes(value).to_string(),
            "color" => color = Some(trim_toml_quotes(value).to_string()),
            "price" => price = value.parse().ok(),
            "rent" => rent = value.parse().ok(),
            _ => {}
        }
    }

    BoardSpace {
        index,
        kind,
        name,
        color,
        price,
        rent,
    }
}

fn trim_toml_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
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
