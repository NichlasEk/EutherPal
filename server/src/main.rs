use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TV_HTML: &str = include_str!("../../web/tv/index.html");
const MOBILE_HTML: &str = include_str!("../../web/mobile/index.html");
const ADMIN_HTML: &str = include_str!("../../web/admin/index.html");
const STYLES_CSS: &str = include_str!("../../web/shared/styles.css");
const APP_JS: &str = include_str!("../../web/shared/app.js");
const TOKEN_BIL_PNG: &[u8] = include_bytes!("../../web/assets/tokens/bil.png");
const TOKEN_HATT_PNG: &[u8] = include_bytes!("../../web/assets/tokens/hatt.png");
const TOKEN_SKEPP_PNG: &[u8] = include_bytes!("../../web/assets/tokens/skepp.png");
const TOKEN_HUND_PNG: &[u8] = include_bytes!("../../web/assets/tokens/hund.png");
const TOKEN_SKO_PNG: &[u8] = include_bytes!("../../web/assets/tokens/sko.png");
const BANK_CAT_PNG: &[u8] = include_bytes!("../../web/assets/bank/bank-cat.png");
const DEFAULT_PREPROMPT: &str = include_str!("../../prompts/supergemma.bank.sv.md");
const DEFAULT_AI_TURN_PROMPT: &str = include_str!("../../prompts/ai-player-turn.sv.md");
const DEFAULT_AI_PROFILES_TOML: &str =
    include_str!("../../prompts/ai-player-profiles.example.toml");
const BOARD_TOML: &str = include_str!("../../rules/board.lindesberg.toml");
const CARDS_TOML: &str = include_str!("../../rules/cards.sv.toml");
const DEFAULT_BANK_RULES_TOML: &str = include_str!("../../rules/bank.rules.toml");
const SETTINGS_PATH: &str = "data/settings.toml";
const GAME_STATE_PATH: &str = "data/game-state.json";
const BANK_RULES_PATH: &str = "rules/bank.rules.toml";
const DEFAULT_MODEL: &str = "supergemma";
const DEFAULT_PLAYER_COUNT: usize = 4;
const MIN_PLAYER_COUNT: usize = 2;
const MAX_PLAYER_COUNT: usize = 5;
const GO_SALARY: i32 = 2000;
const JAIL_FINE: i32 = 500;
const AUCTION_MIN_MS: u128 = 20_000;
const DEFAULT_LLM_TIMEOUT_SECS: u64 = 90;

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
    controller: PlayerController,
    cash: i32,
    position: usize,
    token: Option<String>,
    jailed: bool,
    jail_turns: u8,
    bankrupt: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum PlayerController {
    Human,
    Ai,
}

#[derive(Clone)]
struct BoardSpace {
    index: usize,
    kind: String,
    name: String,
    color: Option<String>,
    price: Option<i32>,
    rent: Option<i32>,
    build_cost: Option<i32>,
    house_rents: Vec<i32>,
    amount: Option<i32>,
    target: Option<usize>,
    card_title: Option<String>,
    card_text: Option<String>,
    card_icon: Option<String>,
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
    started_at_ms: u128,
    last_bid_at_ms: u128,
}

struct BankRules {
    emergency_loan_limit: i32,
    emergency_loan_required_requests: usize,
    player_loan_limit: i32,
    donation_limit: i32,
    trade_cash_limit: i32,
}

#[derive(Clone)]
struct BankProposal {
    id: u64,
    kind: String,
    requester_index: usize,
    counterparty_index: usize,
    awaiting_player_index: usize,
    cash_from_requester: i32,
    cash_from_counterparty: i32,
    spaces_from_requester: Vec<usize>,
    spaces_from_counterparty: Vec<usize>,
    note: String,
}

struct BankAsyncJob {
    player_name: String,
    prompt: String,
    fallback_answer: String,
}

struct AiTurnJob {
    player_name: String,
    prompt: String,
    fallback_decision: AiDecision,
}

struct AiTurnAnswer {
    decision: AiDecision,
    reason: String,
}

#[derive(Clone, Copy, PartialEq)]
enum AiDecision {
    Buy,
    Decline,
    Roll,
    PayJail,
    Liquidate,
    Bankrupt,
    Build,
    Trade,
    AcceptProposal,
    DeclineProposal,
    Wait,
}

#[derive(Clone)]
struct GameCard {
    deck: String,
    id: String,
    title: String,
    text: String,
    icon: String,
    effect: String,
    amount: Option<i32>,
    target: Option<usize>,
}

#[derive(Clone)]
struct DrawnCard {
    deck: String,
    id: String,
    title: String,
    text: String,
    icon: String,
}

#[derive(Clone)]
struct BankChatMessage {
    speaker: String,
    text: String,
    from_bank: bool,
}

struct GameState {
    room_code: String,
    phase: Phase,
    players: Vec<Player>,
    spaces: Vec<BoardSpace>,
    chance_cards: Vec<GameCard>,
    community_cards: Vec<GameCard>,
    next_chance: usize,
    next_community: usize,
    owners: Vec<Option<usize>>,
    buildings: Vec<u8>,
    mortgaged: Vec<bool>,
    selection_order: Vec<usize>,
    selection_cursor: usize,
    current_player_index: usize,
    dice: [u8; 2],
    rng_state: u64,
    pending_offer: Option<PendingOffer>,
    auction: Option<AuctionState>,
    drawn_card: Option<DrawnCard>,
    bank_proposals: Vec<BankProposal>,
    next_bank_proposal_id: u64,
    ai_action_pending: bool,
    bank_chat: Vec<BankChatMessage>,
    events: Vec<String>,
    free_parking_pot: i32,
    bank_message: String,
    bank_ai_status: String,
    ai_turn_source: String,
    ai_turn_thought: String,
    stopped: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    TokenSelection,
    Play,
    Auction,
}

type SharedGame = Arc<Mutex<GameState>>;

fn main() -> std::io::Result<()> {
    let bind_addr = env::var("EUTHERPAL_BIND").unwrap_or_else(|_| "127.0.0.1:8793".to_string());
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
        ("GET", "/health") => {
            let game = game.lock().expect("game state lock");
            json(200, &health_json(&game))
        }
        ("GET", "/api/game") | ("GET", "/api/game/mock") => {
            let (body, should_spawn_ai) = {
                let mut game = game.lock().expect("game state lock");
                let should_spawn_ai = game.queue_ai_action_if_needed();
                (game.to_json(), should_spawn_ai)
            };
            if should_spawn_ai {
                spawn_ai_action(Arc::clone(&game));
            }
            json(200, &body)
        }
        ("GET", "/api/settings") => json(200, &load_settings().to_json()),
        ("POST", "/api/settings") => {
            let mut settings = load_settings();
            settings.model = form_value(&body, "model").unwrap_or(settings.model);
            settings.preprompt = form_value(&body, "preprompt").unwrap_or(settings.preprompt);
            settings.ai_turn_prompt =
                form_value(&body, "aiTurnPrompt").unwrap_or(settings.ai_turn_prompt);
            settings.ai_profiles_toml =
                form_value(&body, "aiProfilesToml").unwrap_or(settings.ai_profiles_toml);
            match save_settings(&settings) {
                Ok(()) => json(200, &settings.to_json()),
                Err(error) => json(
                    500,
                    &format!("{{\"error\":\"{}\"}}", escape_json(&error.to_string())),
                ),
            }
        }
        ("POST", "/api/game/new") => {
            let player_count = form_value(&body, "players")
                .or_else(|| query_value(query, "players"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(DEFAULT_PLAYER_COUNT);
            let ai_count = form_value(&body, "aiPlayers")
                .or_else(|| query_value(query, "aiPlayers"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let ai_names = form_value(&body, "aiNames")
                .or_else(|| query_value(query, "aiNames"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            *game = GameState::new_with_ai_players(player_count, ai_count, &ai_names);
            json(200, &game.to_json())
        }
        ("POST", "/api/game/save") => {
            let mut game = game.lock().expect("game state lock");
            match game.save_to_disk() {
                Ok(()) => {
                    game.bank_message = format!("Spelet sparades till {GAME_STATE_PATH}.");
                    game.push_event("Spelet sparades.".to_string());
                    json(200, &game.to_json())
                }
                Err(error) => json(
                    500,
                    &format!("{{\"error\":\"{}\"}}", escape_json(&error.to_string())),
                ),
            }
        }
        ("POST", "/api/game/load") => {
            let mut game = game.lock().expect("game state lock");
            match GameState::load_from_disk() {
                Ok(loaded) => {
                    *game = loaded;
                    game.bank_message = format!("Spelet laddades från {GAME_STATE_PATH}.");
                    game.push_event("Spelet laddades.".to_string());
                    json(200, &game.to_json())
                }
                Err(error) => json(
                    500,
                    &format!("{{\"error\":\"{}\"}}", escape_json(&error.to_string())),
                ),
            }
        }
        ("POST", "/api/game/demo") => {
            let mut game = game.lock().expect("game state lock");
            *game = GameState::demo();
            json(200, &game.to_json())
        }
        ("POST", "/api/game/stop") => {
            let mut game = game.lock().expect("game state lock");
            game.stop_game();
            json(200, &game.to_json())
        }
        ("POST", "/api/game/admin-adjust") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let cash_delta = form_value(&body, "cashDelta")
                .or_else(|| query_value(query, "cashDelta"))
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let position = form_value(&body, "position")
                .or_else(|| query_value(query, "position"))
                .and_then(|value| value.parse::<usize>().ok());
            let mut game = game.lock().expect("game state lock");
            game.admin_adjust_player(&player, cash_delta, position);
            json(200, &game.to_json())
        }
        ("POST", "/api/game/select-token") => {
            let token = form_value(&body, "token")
                .or_else(|| query_value(query, "token"))
                .unwrap_or_default();
            let player_name = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.select_token(&token, &player_name);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/roll") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.roll_current_player(&player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/buy") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.buy_pending_property(&player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/decline") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.decline_pending_property(&player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/build") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let space_index = form_value(&body, "spaceIndex")
                .or_else(|| query_value(query, "spaceIndex"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.build_property(space_index, &player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/sell-building") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let space_index = form_value(&body, "spaceIndex")
                .or_else(|| query_value(query, "spaceIndex"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.sell_building(space_index, &player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/mortgage") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let space_index = form_value(&body, "spaceIndex")
                .or_else(|| query_value(query, "spaceIndex"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.mortgage_property(space_index, &player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/unmortgage") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let space_index = form_value(&body, "spaceIndex")
                .or_else(|| query_value(query, "spaceIndex"))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.unmortgage_property(space_index, &player);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/pay-jail") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.pay_jail_fine(&player);
            }
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
            if !game.reject_stopped_action() {
                game.place_auction_bid(&player, amount);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/game/auction/finish") => {
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.finish_auction();
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/bank/chat") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let message = form_value(&body, "message")
                .or_else(|| query_value(query, "message"))
                .unwrap_or_default();
            let (body, async_job) = {
                let mut game_state = game.lock().expect("game state lock");
                let async_job = game_state.ask_bank(&player, &message);
                (game_state.to_json(), async_job)
            };
            if let Some(job) = async_job {
                spawn_bank_answer(Arc::clone(&game), job);
            }
            json(200, &body)
        }
        ("POST", "/api/bank/proposal/accept") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let proposal_id = form_value(&body, "proposalId")
                .or_else(|| query_value(query, "proposalId"))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.accept_bank_proposal(&player, proposal_id);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/bank/proposal/decline") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let proposal_id = form_value(&body, "proposalId")
                .or_else(|| query_value(query, "proposalId"))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.decline_bank_proposal(&player, proposal_id);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/bank/proposal/counter") => {
            let player = form_value(&body, "player")
                .or_else(|| query_value(query, "player"))
                .unwrap_or_default();
            let proposal_id = form_value(&body, "proposalId")
                .or_else(|| query_value(query, "proposalId"))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let amount = form_value(&body, "amount")
                .or_else(|| query_value(query, "amount"))
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let mut game = game.lock().expect("game state lock");
            if !game.reject_stopped_action() {
                game.counter_bank_proposal(&player, proposal_id, amount);
            }
            json(200, &game.to_json())
        }
        ("POST", "/api/bank/admin-message") => {
            let message = form_value(&body, "message")
                .or_else(|| query_value(query, "message"))
                .unwrap_or_default();
            let mut game = game.lock().expect("game state lock");
            game.admin_bank_message(&message);
            json(200, &game.to_json())
        }
        ("GET", "/tv") | ("GET", "/tv/") => html(200, TV_HTML),
        ("GET", "/mobile") | ("GET", "/mobile/") => html(200, MOBILE_HTML),
        ("GET", "/admin") | ("GET", "/admin/") => html(200, ADMIN_HTML),
        ("GET", "/assets/styles.css") => asset(200, "text/css; charset=utf-8", STYLES_CSS),
        ("GET", "/assets/app.js") => asset(200, "application/javascript; charset=utf-8", APP_JS),
        ("GET", "/assets/tokens/bil.png") => binary_asset(200, "image/png", TOKEN_BIL_PNG),
        ("GET", "/assets/tokens/hatt.png") => binary_asset(200, "image/png", TOKEN_HATT_PNG),
        ("GET", "/assets/tokens/skepp.png") => binary_asset(200, "image/png", TOKEN_SKEPP_PNG),
        ("GET", "/assets/tokens/hund.png") => binary_asset(200, "image/png", TOKEN_HUND_PNG),
        ("GET", "/assets/tokens/sko.png") => binary_asset(200, "image/png", TOKEN_SKO_PNG),
        ("GET", "/assets/bank/bank-cat.png") => binary_asset(200, "image/png", BANK_CAT_PNG),
        _ => html(404, "<h1>404</h1><p>Sidan finns inte.</p>"),
    };

    stream.write_all(&response)?;
    stream.flush()?;
    Ok(())
}

fn spawn_bank_answer(game: SharedGame, job: BankAsyncJob) {
    thread::spawn(move || {
        let result = llm_answer_for_prompt(&job.prompt);
        let mut game = game.lock().expect("game state lock");
        let (answer, status) = match result {
            Ok(answer) => (answer, llm_config_label()),
            Err(error) => (job.fallback_answer, format!("mock fallback: {error}")),
        };
        game.finish_async_bank_answer(&job.player_name, &answer, &status);
    });
}

fn spawn_ai_action(game: SharedGame) {
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(650));
        let ai_job = {
            let mut game = game.lock().expect("game state lock");
            if game.phase == Phase::TokenSelection || game.phase == Phase::Auction {
                game.run_ai_action();
                game.ai_action_pending = false;
                return;
            }
            game.prepare_ai_turn_job()
        };
        let Some(job) = ai_job else {
            let mut game = game.lock().expect("game state lock");
            game.ai_action_pending = false;
            return;
        };
        let (decision, reason, status) = match llm_ai_decision_for_prompt(&job.prompt) {
            Ok(answer) => (
                answer
                    .decision
                    .constrained_for_fallback(job.fallback_decision),
                answer.reason,
                llm_config_label(),
            ),
            Err(error) => (
                job.fallback_decision,
                format!("Fallback: {error}"),
                format!("ai fallback: {error}"),
            ),
        };
        let mut game = game.lock().expect("game state lock");
        game.bank_ai_status = status;
        game.apply_ai_turn_decision(&job.player_name, decision, &reason);
        game.ai_action_pending = false;
    });
}

struct Settings {
    model: String,
    preprompt: String,
    ai_turn_prompt: String,
    ai_profiles_toml: String,
}

impl Settings {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            preprompt: DEFAULT_PREPROMPT.trim().to_string(),
            ai_turn_prompt: DEFAULT_AI_TURN_PROMPT.trim().to_string(),
            ai_profiles_toml: DEFAULT_AI_PROFILES_TOML.trim().to_string(),
        }
    }

    fn to_json(&self) -> String {
        format!(
            "{{\"model\":\"{}\",\"preprompt\":\"{}\",\"aiTurnPrompt\":\"{}\",\"aiProfilesToml\":\"{}\",\"path\":\"{}\"}}",
            escape_json(&self.model),
            escape_json(&self.preprompt),
            escape_json(&self.ai_turn_prompt),
            escape_json(&self.ai_profiles_toml),
            SETTINGS_PATH
        )
    }

    fn to_toml(&self) -> String {
        format!(
            "[llm]\nmodel = \"{}\"\n\n[bank]\npreprompt = \"\"\"\n{}\n\"\"\"\n\n[ai_players]\nturn_prompt = \"\"\"\n{}\n\"\"\"\nprofiles_toml = '''\n{}\n'''\n",
            escape_toml_string(&self.model),
            escape_toml_multiline(&self.preprompt),
            escape_toml_multiline(&self.ai_turn_prompt),
            escape_toml_literal_multiline(&self.ai_profiles_toml)
        )
    }
}

fn llm_model_name(settings: &Settings) -> String {
    if settings.model == "supergemma" {
        env::var("EUTHERPAL_LLM_MODEL")
            .unwrap_or_else(|_| "supergemma4-26b-free:latest".to_string())
    } else {
        settings.model.clone()
    }
}

fn llm_config_label() -> String {
    let settings = load_settings();
    let model = llm_model_name(&settings);
    match env::var("EUTHERPAL_LLM_URL") {
        Ok(url) if !url.trim().is_empty() && url != "mock" => format!("llm:{model}"),
        _ => format!("mock:{model}"),
    }
}

fn llm_answer_for_prompt(prompt: &str) -> Result<String, String> {
    llm_answer_for_prompt_mode(prompt, false, 180).map(|answer| clean_chat_message(&answer))
}

fn llm_answer_for_prompt_mode(
    prompt: &str,
    json_mode: bool,
    num_predict: u16,
) -> Result<String, String> {
    let llm_url =
        env::var("EUTHERPAL_LLM_URL").map_err(|_| "EUTHERPAL_LLM_URL saknas".to_string())?;
    if llm_url.trim().is_empty() || llm_url == "mock" {
        return Err("LLM är satt till mock".to_string());
    }
    let settings = load_settings();
    let model = llm_model_name(&settings);
    let answer = call_ollama_generate(&llm_url, &model, prompt, json_mode, num_predict)
        .map_err(|error| llm_io_error_message(&error))
        .map(|answer| answer.trim().to_string())?;
    if answer.is_empty() {
        Err("LLM gav tomt svar".to_string())
    } else {
        Ok(answer)
    }
}

fn llm_timeout_secs() -> u64 {
    env::var("EUTHERPAL_LLM_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LLM_TIMEOUT_SECS)
        .clamp(10, 240)
}

fn llm_io_error_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            format!("LLM hann inte svara inom {} sekunder", llm_timeout_secs())
        }
        _ => error.to_string(),
    }
}

fn llm_ai_decision_for_prompt(prompt: &str) -> Result<AiTurnAnswer, String> {
    let answer = llm_answer_for_prompt_mode(prompt, true, 96)?;
    AiTurnAnswer::from_llm_answer(&answer)
        .ok_or_else(|| "LLM gav inget giltigt AI-drag".to_string())
}

fn health_json(game: &GameState) -> String {
    let settings = load_settings();
    let model = llm_model_name(&settings);
    let endpoint = env::var("EUTHERPAL_LLM_URL").unwrap_or_else(|_| "mock".to_string());
    let mode = if endpoint.trim().is_empty() || endpoint == "mock" {
        "mock"
    } else {
        "ollama"
    };
    format!(
        "{{\"status\":\"ok\",\"service\":\"eutherpal\",\"ai\":\"{}\",\"llm\":{{\"mode\":\"{}\",\"model\":\"{}\",\"endpointConfigured\":{},\"lastBankStatus\":\"{}\"}}}}",
        escape_json(&game.bank_ai_status),
        mode,
        escape_json(&model),
        if mode == "ollama" { "true" } else { "false" },
        escape_json(&game.bank_ai_status)
    )
}

impl GameState {
    fn new() -> Self {
        Self::new_with_player_count(DEFAULT_PLAYER_COUNT)
    }

    fn new_with_ai_players(player_count: usize, ai_count: usize, ai_names: &str) -> Self {
        let mut game = Self::new_with_player_count(player_count);
        game.configure_ai_players(ai_count, ai_names);
        game.refresh_selection_message();
        game
    }

    fn new_with_player_count(player_count: usize) -> Self {
        let spaces = parse_board_spaces(BOARD_TOML);
        let (chance_cards, community_cards) = parse_game_cards(CARDS_TOML);
        let owners = vec![None; spaces.len()];
        let buildings = vec![0; spaces.len()];
        let mortgaged = vec![false; spaces.len()];
        let player_count = player_count.clamp(MIN_PLAYER_COUNT, MAX_PLAYER_COUNT);
        let players = (1..=player_count)
            .map(|number| Player::new(&format!("Spelare {number}")))
            .collect::<Vec<_>>();
        let first = random_index(players.len());
        let mut selection_order = Vec::new();
        for offset in 0..players.len() {
            selection_order.push((first + offset) % players.len());
        }
        let first_name = players[first].name.clone();
        let rng_state = seed_rng(player_count as u64);

        Self {
            room_code: "PAL-001".to_string(),
            phase: Phase::TokenSelection,
            players,
            spaces,
            chance_cards,
            community_cards,
            next_chance: 0,
            next_community: 0,
            owners,
            buildings,
            mortgaged,
            selection_order,
            selection_cursor: 0,
            current_player_index: first,
            dice: [0, 0],
            rng_state,
            pending_offer: None,
            auction: None,
            drawn_card: None,
            bank_proposals: Vec::new(),
            next_bank_proposal_id: 1,
            ai_action_pending: false,
            bank_chat: vec![BankChatMessage {
                speaker: "Banken".to_string(),
                text: "Jag är redo som bank och spelledare. Skriv i mobilen om du vill fråga om regler, köp, hyra eller din position.".to_string(),
                from_bank: true,
            }],
            events: vec![format!("{first_name} börjar och väljer pjäs först.")],
            free_parking_pot: 0,
            bank_message: format!(
                "{first_name} börjar och väljer pjäs först. Välj en av de fem klassiska pjäserna."
            ),
            bank_ai_status: llm_config_label(),
            ai_turn_source: "idle".to_string(),
            ai_turn_thought: String::new(),
            stopped: false,
        }
    }

    fn configure_ai_players(&mut self, ai_count: usize, ai_names: &str) {
        let ai_count = ai_count.min(self.players.len());
        let human_count = self.players.len().saturating_sub(ai_count);
        let names = parse_ai_names(ai_names);
        for (index, player) in self.players.iter_mut().enumerate() {
            player.controller = if index >= human_count {
                PlayerController::Ai
            } else {
                PlayerController::Human
            };
            if player.controller == PlayerController::Ai {
                let ai_number = index - human_count;
                player.name = names
                    .get(ai_number)
                    .cloned()
                    .unwrap_or_else(|| default_ai_name(ai_number));
            }
        }
    }

    fn refresh_selection_message(&mut self) {
        if self.phase != Phase::TokenSelection || self.selection_order.is_empty() {
            return;
        }
        let first = self.selection_order[self.selection_cursor];
        let first_name = self.players[first].name.clone();
        let ai_count = self
            .players
            .iter()
            .filter(|player| player.controller == PlayerController::Ai)
            .count();
        let human_count = self.players.len().saturating_sub(ai_count);
        let controller_text = if ai_count > 0 {
            let human_label = if human_count == 1 {
                "människa"
            } else {
                "människor"
            };
            format!(" {human_count} {human_label} och {ai_count} AI-spelare är med.")
        } else {
            String::new()
        };
        self.events = vec![format!("{first_name} börjar och väljer pjäs först.")];
        self.bank_message = format!("{first_name} börjar och väljer pjäs först.{controller_text}");
    }

    fn demo() -> Self {
        let mut game = Self::new_with_player_count(4);
        let names = ["Maja", "Noel", "Iris", "Sam"];
        let tokens = ["bil", "hatt", "skepp", "hund"];
        for index in 0..game.players.len() {
            game.players[index].name = names[index].to_string();
            game.players[index].token = Some(tokens[index].to_string());
        }
        game.phase = Phase::Play;
        game.selection_cursor = game.players.len();
        game.current_player_index = 0;
        game.players[0].position = 11;
        game.players[1].position = 10;
        game.players[1].jailed = true;
        game.players[1].jail_turns = 1;
        game.players[2].position = 5;
        game.players[3].position = 37;
        for index in [1, 3, 5, 6, 8, 9] {
            game.owners[index] = Some(0);
        }
        game.owners[12] = Some(1);
        game.owners[27] = Some(1);
        game.owners[21] = Some(2);
        game.owners[23] = Some(2);
        game.owners[24] = Some(2);
        game.buildings[1] = 1;
        game.buildings[3] = 1;
        game.buildings[21] = 2;
        game.buildings[23] = 2;
        game.mortgaged[5] = true;
        game.dice = [3, 4];
        game.free_parking_pot = 1300;
        game.events = vec![
            "Demo-läge startat.".to_string(),
            "Maja äger bruna gruppen och Lindesberg C är intecknad.".to_string(),
            "Noel sitter i fängelse.".to_string(),
        ];
        game.bank_message =
            "Demo-läge är laddat. Testa inteckning, fängelse, byggnader och bankchatt.".to_string();
        game.bank_chat = vec![BankChatMessage {
            speaker: "Banken".to_string(),
            text: "Demo-läge laddat. Jag kan hjälpa till med regler och läge.".to_string(),
            from_bank: true,
        }];
        game
    }

    fn save_to_disk(&self) -> std::io::Result<()> {
        if let Some(parent) = Path::new(GAME_STATE_PATH).parent() {
            fs::create_dir_all(parent)?;
        }
        let snapshot = self.to_snapshot();
        let json = format!(
            "{{\"format\":\"eutherpal-v1\",\"state\":\"{}\"}}\n",
            escape_json(&snapshot)
        );
        fs::write(GAME_STATE_PATH, json)
    }

    fn load_from_disk() -> std::io::Result<Self> {
        let json = fs::read_to_string(GAME_STATE_PATH)?;
        let snapshot = json_string_field(&json, "state").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "saknar state i sparfilen")
        })?;
        Self::from_snapshot(&snapshot).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "kunde inte läsa sparfilen")
        })
    }

    fn admin_adjust_player(&mut self, player_name: &str, cash_delta: i32, position: Option<usize>) {
        let player_name = clean_player_name(player_name);
        let Some(index) = self
            .players
            .iter()
            .position(|player| player.name == player_name)
        else {
            self.bank_message = "Adminjustering misslyckades: okänd spelare.".to_string();
            return;
        };
        self.players[index].cash += cash_delta;
        if let Some(position) = position {
            if position < self.spaces.len() {
                self.players[index].position = position;
            }
        }
        self.players[index].bankrupt = false;
        let name = self.players[index].name.clone();
        self.bank_message = format!("Admin justerade {name}: {cash_delta:+} kr.");
        self.push_event(format!("Admin justerade {name}."));
    }

    fn stop_game(&mut self) {
        self.stopped = true;
        self.ai_action_pending = false;
        self.ai_turn_source = "stoppad".to_string();
        self.ai_turn_thought = "Spelet är stoppat av admin.".to_string();
        self.bank_message =
            "Spelet är stoppat av admin. Starta nytt spel för att fortsätta.".to_string();
        self.push_event("Admin stoppade pågående spel.".to_string());
        self.push_bank_message(
            "Banken",
            "Spelet är stoppat av admin. Inga nya spelhandlingar körs.",
            true,
        );
    }

    fn reject_stopped_action(&mut self) -> bool {
        if !self.stopped {
            return false;
        }
        self.bank_message =
            "Spelet är stoppat av admin. Starta nytt spel om ni vill fortsätta.".to_string();
        true
    }

    fn to_snapshot(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "meta\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.room_code,
            self.phase.as_str(),
            self.selection_cursor,
            self.current_player_index,
            self.dice[0],
            self.dice[1],
            snapshot_escape(&self.bank_message),
            self.stopped
        ));
        lines.push(format!(
            "selection\t{}",
            self.selection_order
                .iter()
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        lines.push(format!(
            "cards\t{}\t{}",
            self.next_chance, self.next_community
        ));
        lines.push(format!("pot\t{}", self.free_parking_pot));
        lines.push(format!("proposal_next\t{}", self.next_bank_proposal_id));
        for proposal in &self.bank_proposals {
            lines.push(format!(
                "proposal\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                proposal.id,
                snapshot_escape(&proposal.kind),
                proposal.requester_index,
                proposal.counterparty_index,
                proposal.awaiting_player_index,
                proposal.cash_from_requester,
                proposal.cash_from_counterparty,
                snapshot_usize_list(&proposal.spaces_from_requester),
                snapshot_usize_list(&proposal.spaces_from_counterparty),
                snapshot_escape(&proposal.note)
            ));
        }
        if let Some(offer) = &self.pending_offer {
            lines.push(format!(
                "pending\t{}\t{}",
                offer.player_index, offer.space_index
            ));
        }
        if let Some(auction) = &self.auction {
            lines.push(format!(
                "auction\t{}\t{}\t{}\t{}\t{}\t{}",
                auction.space_index,
                auction.seller_turn_index,
                auction.highest_bid,
                auction
                    .highest_bidder
                    .map(|index| index as i32)
                    .unwrap_or(-1),
                auction.started_at_ms,
                auction.last_bid_at_ms
            ));
        }
        for player in &self.players {
            lines.push(format!(
                "player\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                snapshot_escape(&player.name),
                player.cash,
                player.position,
                snapshot_escape(player.token.as_deref().unwrap_or("")),
                player.jailed,
                player.jail_turns,
                player.bankrupt,
                player.controller.as_str()
            ));
        }
        for index in 0..self.spaces.len() {
            lines.push(format!(
                "asset\t{}\t{}\t{}\t{}",
                index,
                self.owners[index].map(|owner| owner as i32).unwrap_or(-1),
                self.buildings[index],
                self.mortgaged[index]
            ));
        }
        lines.push(format!("rng\t{}", self.rng_state));
        for event in &self.events {
            lines.push(format!("event\t{}", snapshot_escape(event)));
        }
        for message in &self.bank_chat {
            lines.push(format!(
                "chat\t{}\t{}\t{}",
                snapshot_escape(&message.speaker),
                message.from_bank,
                snapshot_escape(&message.text)
            ));
        }
        lines.join("\n")
    }

    fn from_snapshot(snapshot: &str) -> Option<Self> {
        let player_count = snapshot
            .lines()
            .filter(|line| line.starts_with("player\t"))
            .count()
            .clamp(MIN_PLAYER_COUNT, MAX_PLAYER_COUNT);
        let mut game = Self::new_with_player_count(player_count);
        game.events.clear();
        game.bank_chat.clear();

        let mut player_index = 0usize;
        for line in snapshot.lines() {
            let parts = line.split('\t').collect::<Vec<_>>();
            match parts.first().copied().unwrap_or("") {
                "meta" if parts.len() >= 8 => {
                    game.room_code = parts[1].to_string();
                    game.phase = Phase::from_str(parts[2]);
                    game.selection_cursor = parts[3].parse().ok()?;
                    game.current_player_index = parts[4].parse().ok()?;
                    game.dice = [parts[5].parse().ok()?, parts[6].parse().ok()?];
                    game.bank_message = snapshot_unescape(parts[7]);
                    game.stopped = parts.get(8).map(|value| parse_bool(value)).unwrap_or(false);
                }
                "selection" if parts.len() >= 2 => {
                    game.selection_order = parts[1]
                        .split(',')
                        .filter_map(|value| value.parse::<usize>().ok())
                        .collect();
                }
                "cards" if parts.len() >= 3 => {
                    game.next_chance = parts[1].parse().ok()?;
                    game.next_community = parts[2].parse().ok()?;
                }
                "rng" if parts.len() >= 2 => {
                    game.rng_state = parts[1].parse().ok()?;
                }
                "pot" if parts.len() >= 2 => {
                    game.free_parking_pot = parts[1].parse().ok()?;
                }
                "proposal_next" if parts.len() >= 2 => {
                    game.next_bank_proposal_id = parts[1].parse().ok()?;
                }
                "proposal" if parts.len() >= 10 => {
                    let proposal = BankProposal {
                        id: parts[1].parse().ok()?,
                        kind: snapshot_unescape(parts[2]),
                        requester_index: parts[3].parse().ok()?,
                        counterparty_index: parts[4].parse().ok()?,
                        awaiting_player_index: parts[5].parse().ok()?,
                        cash_from_requester: parts[6].parse().ok()?,
                        cash_from_counterparty: parts[7].parse().ok()?,
                        spaces_from_requester: parse_snapshot_usize_list(parts[8]),
                        spaces_from_counterparty: parse_snapshot_usize_list(parts[9]),
                        note: parts
                            .get(10)
                            .map(|value| snapshot_unescape(value))
                            .unwrap_or_default(),
                    };
                    game.next_bank_proposal_id = game.next_bank_proposal_id.max(proposal.id + 1);
                    game.bank_proposals.push(proposal);
                }
                "pending" if parts.len() >= 3 => {
                    game.pending_offer = Some(PendingOffer {
                        player_index: parts[1].parse().ok()?,
                        space_index: parts[2].parse().ok()?,
                    });
                }
                "auction" if parts.len() >= 5 => {
                    let bidder = parts[4].parse::<i32>().ok()?;
                    let fallback_time = now_millis();
                    game.auction = Some(AuctionState {
                        space_index: parts[1].parse().ok()?,
                        seller_turn_index: parts[2].parse().ok()?,
                        highest_bid: parts[3].parse().ok()?,
                        highest_bidder: (bidder >= 0).then_some(bidder as usize),
                        started_at_ms: parts
                            .get(5)
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(fallback_time),
                        last_bid_at_ms: parts
                            .get(6)
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(fallback_time),
                    });
                }
                "player" if parts.len() >= 8 && player_index < game.players.len() => {
                    game.players[player_index].name = snapshot_unescape(parts[1]);
                    game.players[player_index].cash = parts[2].parse().ok()?;
                    game.players[player_index].position = parts[3].parse().ok()?;
                    let token = snapshot_unescape(parts[4]);
                    game.players[player_index].token = (!token.is_empty()).then_some(token);
                    game.players[player_index].jailed = parse_bool(parts[5]);
                    game.players[player_index].jail_turns = parts[6].parse().ok()?;
                    game.players[player_index].bankrupt = parse_bool(parts[7]);
                    game.players[player_index].controller = parts
                        .get(8)
                        .map(|value| PlayerController::from_str(value))
                        .unwrap_or(PlayerController::Human);
                    player_index += 1;
                }
                "asset" if parts.len() >= 5 => {
                    let index = parts[1].parse::<usize>().ok()?;
                    if index < game.spaces.len() {
                        let owner = parts[2].parse::<i32>().ok()?;
                        game.owners[index] = (owner >= 0).then_some(owner as usize);
                        game.buildings[index] = parts[3].parse().ok()?;
                        game.mortgaged[index] = parse_bool(parts[4]);
                    }
                }
                "event" if parts.len() >= 2 => game.events.push(snapshot_unescape(parts[1])),
                "chat" if parts.len() >= 4 => game.bank_chat.push(BankChatMessage {
                    speaker: snapshot_unescape(parts[1]),
                    from_bank: parse_bool(parts[2]),
                    text: snapshot_unescape(parts[3]),
                }),
                _ => {}
            }
        }
        if game.selection_order.is_empty() {
            game.selection_order = (0..game.players.len()).collect();
        }
        game.current_player_index %= game.players.len();
        game.selection_cursor = game
            .selection_cursor
            .min(game.players.len().saturating_sub(1));
        Some(game)
    }

    fn select_token(&mut self, token: &str, requested_name: &str) {
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
        let chosen_name = clean_player_name(requested_name);
        if chosen_name.is_empty() {
            self.bank_message = "Skriv in ditt namn innan du väljer pjäs.".to_string();
            return;
        }
        if self
            .players
            .iter()
            .enumerate()
            .any(|(index, player)| index != player_index && player.name == chosen_name)
        {
            self.bank_message = "Det namnet används redan av en annan spelare.".to_string();
            return;
        }

        self.players[player_index].name = chosen_name;
        let player_name = self.players[player_index].name.clone();
        let token_label = token_label(token);
        self.players[player_index].token = Some(token.to_string());
        self.selection_cursor += 1;
        self.push_event(format!("{player_name} valde {token_label}."));

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

    fn roll_current_player(&mut self, player_name: &str) {
        if self.phase != Phase::Play {
            self.bank_message =
                "Alla spelare måste välja pjäs innan första tärningsslaget.".to_string();
            return;
        }
        if !self.is_authorized_current_player(player_name) {
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
        if self.players[self.current_player_index].cash < 0 {
            let player_name = self.players[self.current_player_index].name.clone();
            let debt = -self.players[self.current_player_index].cash;
            self.bank_message = format!(
                "{player_name} ligger {debt} kr back. Sälj byggnader, inteckna, ta emot gåva/byte eller prata med banken om konkurs innan nästa tärningsslag."
            );
            return;
        }

        self.dice = [self.roll_die(), self.roll_die()];
        self.drawn_card = None;
        if self.players[self.current_player_index].jailed {
            let player_name = self.players[self.current_player_index].name.clone();
            if self.dice[0] == self.dice[1] {
                self.players[self.current_player_index].jailed = false;
                self.players[self.current_player_index].jail_turns = 0;
                self.push_event(format!("{player_name} slog dubbel och lämnar fängelset."));
                self.bank_message = format!(
                    "{player_name} slog {} + {} och lämnar fängelset.",
                    self.dice[0], self.dice[1]
                );
            } else {
                self.players[self.current_player_index].jail_turns += 1;
                if self.players[self.current_player_index].jail_turns >= 3 {
                    self.pay_player_to_free_parking_pot(self.current_player_index, JAIL_FINE);
                    self.players[self.current_player_index].jailed = false;
                    self.players[self.current_player_index].jail_turns = 0;
                    self.bank_message = format!(
                        "{player_name} slog inte dubbel tredje gången och betalar {JAIL_FINE} kr till Fri parkering-potten."
                    );
                    self.push_event(format!(
                            "{player_name} betalade {JAIL_FINE} kr till Fri parkering-potten efter tre misslyckade fängelsekast."
                        ));
                    self.resolve_negative_cash(self.current_player_index);
                } else {
                    self.bank_message = format!(
                        "{player_name} slog {} + {} i fängelset. Ingen dubbel, turen går vidare.",
                        self.dice[0], self.dice[1]
                    );
                    self.push_event(format!("{player_name} blev kvar i fängelset."));
                    self.advance_to_next_active_player();
                    return;
                }
            }
        }
        let steps = (self.dice[0] + self.dice[1]) as usize;
        let player_index = self.current_player_index;
        let old_position = self.players[player_index].position;
        let board_size = self.spaces.len();
        let passed_go = old_position + steps >= board_size;
        self.players[player_index].position = (old_position + steps) % board_size;
        if passed_go {
            self.players[player_index].cash += GO_SALARY;
        }
        let player_name = self.players[player_index].name.clone();

        let landed_index = self.players[player_index].position;
        let landed = self.spaces[landed_index].clone();
        self.bank_message = format!(
            "{} slog {} + {} och går till {}.",
            player_name, self.dice[0], self.dice[1], landed.name
        );
        self.push_event(format!(
            "{} slog {} + {} och gick till {}.",
            player_name, self.dice[0], self.dice[1], landed.name
        ));
        if passed_go {
            self.bank_message
                .push_str(&format!(" Banken betalar {GO_SALARY} kr för Gå."));
            self.push_event(format!(
                "{player_name} fick {GO_SALARY} kr av banken för Gå."
            ));
        }
        let mut force_end_turn = false;

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
                    self.push_event(format!("{} är ledig för {} kr.", landed.name, price));
                    return;
                }
                Some(owner_index) if owner_index != self.current_player_index => {
                    let rent = self.rent_for_space(landed_index);
                    let payer_name = self.players[self.current_player_index].name.clone();
                    let owner_name = self.players[owner_index].name.clone();
                    self.players[self.current_player_index].cash -= rent;
                    self.players[owner_index].cash += rent;
                    self.push_event(format!(
                        "{payer_name} betalade {} kr i hyra till {owner_name}.",
                        rent
                    ));
                    self.resolve_negative_cash(self.current_player_index);
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

        if landed.kind == "tax" {
            let amount = landed.amount.unwrap_or(0);
            self.pay_player_to_free_parking_pot(self.current_player_index, amount);
            self.bank_message
                .push_str(&format!(" Betala {} kr till Fri parkering-potten.", amount));
            self.push_event(format!(
                "{player_name} betalade {} kr i {} till Fri parkering-potten.",
                amount, landed.name
            ));
            self.resolve_negative_cash(self.current_player_index);
        } else if landed.kind == "free_parking" {
            let payout = self.collect_free_parking_pot(self.current_player_index);
            if payout > 0 {
                self.bank_message
                    .push_str(&format!(" Fri parkering betalar ut potten: {} kr.", payout));
                self.push_event(format!(
                    "{player_name} fick {} kr från Fri parkering-potten.",
                    payout
                ));
            } else {
                self.bank_message
                    .push_str(" Fri parkering-potten är tom just nu.");
            }
        } else if landed.kind == "go_to_jail" {
            let target = landed.target.unwrap_or(10);
            self.players[self.current_player_index].position = target;
            self.players[self.current_player_index].jailed = true;
            self.players[self.current_player_index].jail_turns = 0;
            let target_name = self
                .spaces
                .get(target)
                .map(|space| space.name.clone())
                .unwrap_or_else(|| "Fängelse".to_string());
            self.bank_message.push_str(&format!(
                " {} går direkt till {}.",
                player_name, target_name
            ));
            self.push_event(format!("{player_name} gick direkt till {target_name}."));
            force_end_turn = true;
        } else if landed.kind == "chance" || landed.kind == "community" {
            if self.draw_and_apply_card(&landed.kind, &player_name) {
                force_end_turn = true;
            }
        }

        if force_end_turn {
            self.advance_to_next_active_player();
        } else {
            self.finish_turn_after_roll();
        }
    }

    fn buy_pending_property(&mut self, player_name: &str) {
        if !self.is_authorized_current_player(player_name) {
            return;
        }
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
        self.resolve_negative_cash(offer.player_index);
        self.bank_message = format!(
            "{} köper {} för {} kr.",
            self.players[offer.player_index].name, space.name, price
        );
        self.push_event(format!(
            "{} köpte {} för {} kr.",
            self.players[offer.player_index].name, space.name, price
        ));
        self.finish_turn_after_roll();
    }

    fn decline_pending_property(&mut self, player_name: &str) {
        if !self.is_authorized_current_player(player_name) {
            return;
        }
        let Some(offer) = self.pending_offer.take() else {
            self.bank_message = "Det finns ingen köpfråga att avstå.".to_string();
            return;
        };
        let space = self.spaces[offer.space_index].clone();
        let started_at_ms = now_millis();
        self.phase = Phase::Auction;
        self.auction = Some(AuctionState {
            space_index: offer.space_index,
            seller_turn_index: offer.player_index,
            highest_bid: 0,
            highest_bidder: None,
            started_at_ms,
            last_bid_at_ms: started_at_ms,
        });
        self.bank_message = format!(
            "{} avstår från att köpa {}. Auktionen startar på 0 kr.",
            self.players[offer.player_index].name, space.name
        );
        self.push_event(format!(
            "{} avstod från {}. Auktion startar.",
            self.players[offer.player_index].name, space.name
        ));
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
        auction.last_bid_at_ms = now_millis();
        let space_name = self.spaces[auction.space_index].name.clone();
        self.bank_message = format!(
            "{} leder auktionen på {} med {} kr.",
            self.players[player_index].name, space_name, amount
        );
        self.push_event(format!(
            "{} bjöd {} kr på {}.",
            self.players[player_index].name, amount, space_name
        ));
    }

    fn finish_auction(&mut self) {
        let Some(auction_view) = &self.auction else {
            self.bank_message = "Det finns ingen aktiv auktion att avsluta.".to_string();
            return;
        };
        let seconds_left = auction_seconds_left(auction_view);
        if seconds_left > 0 {
            let space_name = self.spaces[auction_view.space_index].name.clone();
            self.bank_message =
                format!("Auktionen på {space_name} är öppen i {seconds_left} sekunder till.");
            return;
        }

        let Some(auction) = self.auction.take() else {
            return;
        };

        let space = self.spaces[auction.space_index].clone();
        if let Some(winner) = auction.highest_bidder {
            self.players[winner].cash -= auction.highest_bid;
            self.owners[auction.space_index] = Some(winner);
            self.resolve_negative_cash(winner);
            self.bank_message = format!(
                "{} vinner auktionen på {} för {} kr.",
                self.players[winner].name, space.name, auction.highest_bid
            );
            self.push_event(format!(
                "{} vann auktionen på {} för {} kr.",
                self.players[winner].name, space.name, auction.highest_bid
            ));
        } else {
            self.bank_message = format!(
                "Ingen lade bud på {}. Rutan är fortfarande ledig.",
                space.name
            );
            self.push_event(format!("Auktionen på {} avslutades utan bud.", space.name));
        }

        self.phase = Phase::Play;
        self.finish_turn_after_roll();
    }

    fn build_property(&mut self, space_index: usize, player_name: &str) {
        if self.phase != Phase::Play {
            self.bank_message = "Byggande kan bara göras när spelet är igång.".to_string();
            return;
        }
        if !self.is_authorized_current_player(player_name) {
            return;
        }
        if self.pending_offer.is_some() || self.auction.is_some() {
            self.bank_message = "Avsluta köpfråga eller auktion innan du bygger.".to_string();
            return;
        }
        if space_index >= self.spaces.len() {
            self.bank_message = "Välj en giltig gata att bygga på.".to_string();
            return;
        }

        let player_index = self.current_player_index;
        let Some(error) = self.build_error(player_index, space_index) else {
            let level = self.buildings[space_index] + 1;
            let cost = self.spaces[space_index].build_cost.unwrap_or(0);
            let player_name = self.players[player_index].name.clone();
            let space_name = self.spaces[space_index].name.clone();
            self.players[player_index].cash -= cost;
            self.buildings[space_index] = level;
            self.resolve_negative_cash(player_index);
            let label = building_label(level);
            self.bank_message =
                format!("{player_name} bygger {label} på {space_name} för {cost} kr.");
            self.push_event(format!(
                "{player_name} byggde {label} på {space_name} för {cost} kr."
            ));
            return;
        };

        self.bank_message = error;
    }

    fn sell_building(&mut self, space_index: usize, player_name: &str) {
        let Some(player_index) = self.player_index_for_asset_action(player_name) else {
            return;
        };
        if space_index >= self.spaces.len() || self.owners[space_index] != Some(player_index) {
            self.bank_message = "Välj en egen gata med byggnad att sälja.".to_string();
            return;
        }
        if self.buildings[space_index] == 0 {
            self.bank_message = "Det finns ingen byggnad att sälja på den gatan.".to_string();
            return;
        }
        let value = self.spaces[space_index].build_cost.unwrap_or(0) / 2;
        self.buildings[space_index] -= 1;
        self.players[player_index].cash += value;
        let player_name = self.players[player_index].name.clone();
        let space_name = self.spaces[space_index].name.clone();
        self.bank_message =
            format!("{player_name} säljer en byggnadsnivå på {space_name} för {value} kr.");
        self.push_event(format!("{player_name} sålde byggnad på {space_name}."));
    }

    fn mortgage_property(&mut self, space_index: usize, player_name: &str) {
        let Some(player_index) = self.player_index_for_asset_action(player_name) else {
            return;
        };
        if space_index >= self.spaces.len() || self.owners[space_index] != Some(player_index) {
            self.bank_message = "Välj en egen fastighet att inteckna.".to_string();
            return;
        }
        if self.mortgaged[space_index] {
            self.bank_message = "Fastigheten är redan intecknad.".to_string();
            return;
        }
        if self.has_buildings_in_group(space_index) {
            self.bank_message = "Sälj byggnader i färggruppen innan du intecknar.".to_string();
            return;
        }
        let value = self.mortgage_value(space_index);
        self.mortgaged[space_index] = true;
        self.players[player_index].cash += value;
        let player_name = self.players[player_index].name.clone();
        let space_name = self.spaces[space_index].name.clone();
        self.bank_message = format!("{player_name} intecknar {space_name} och får {value} kr.");
        self.push_event(format!("{player_name} intecknade {space_name}."));
    }

    fn unmortgage_property(&mut self, space_index: usize, player_name: &str) {
        let Some(player_index) = self.player_index_for_asset_action(player_name) else {
            return;
        };
        if space_index >= self.spaces.len() || self.owners[space_index] != Some(player_index) {
            self.bank_message = "Välj en egen intecknad fastighet att lösa.".to_string();
            return;
        }
        if !self.mortgaged[space_index] {
            self.bank_message = "Fastigheten är inte intecknad.".to_string();
            return;
        }
        let cost = self.unmortgage_cost(space_index);
        if self.players[player_index].cash < cost {
            self.bank_message = format!("Det kostar {cost} kr att lösa inteckningen.");
            return;
        }
        self.players[player_index].cash -= cost;
        self.mortgaged[space_index] = false;
        let player_name = self.players[player_index].name.clone();
        let space_name = self.spaces[space_index].name.clone();
        self.bank_message =
            format!("{player_name} löser inteckningen på {space_name} för {cost} kr.");
        self.push_event(format!("{player_name} löste inteckningen på {space_name}."));
    }

    fn player_index_for_asset_action(&mut self, player_name: &str) -> Option<usize> {
        if self.phase != Phase::Play {
            self.bank_message =
                "Fastighetsåtgärder kan bara göras när spelet är igång.".to_string();
            return None;
        }
        if clean_player_name(player_name).is_empty() {
            return Some(self.current_player_index);
        }
        let Some(player_index) = self.player_index_by_name(player_name) else {
            self.bank_message = "Skriv ditt spelarnamn innan du ändrar fastigheter.".to_string();
            return None;
        };
        if player_index == self.current_player_index || self.players[player_index].cash < 0 {
            return Some(player_index);
        }
        self.bank_message =
            "Du kan bara ändra fastigheter på din tur, eller när du ligger på minus.".to_string();
        None
    }

    fn pay_player_to_free_parking_pot(&mut self, player_index: usize, amount: i32) {
        if amount <= 0 || player_index >= self.players.len() {
            return;
        }
        self.players[player_index].cash -= amount;
        self.free_parking_pot += amount;
    }

    fn collect_free_parking_pot(&mut self, player_index: usize) -> i32 {
        if player_index >= self.players.len() {
            return 0;
        }
        let payout = self.free_parking_pot;
        if payout > 0 {
            self.players[player_index].cash += payout;
            self.free_parking_pot = 0;
        }
        payout
    }

    fn pay_jail_fine(&mut self, player_name: &str) {
        if !self.is_authorized_current_player(player_name) {
            return;
        }
        if !self.players[self.current_player_index].jailed {
            self.bank_message = "Du sitter inte i fängelse.".to_string();
            return;
        }
        if self.players[self.current_player_index].cash < JAIL_FINE {
            self.bank_message =
                format!("Du behöver {JAIL_FINE} kr för att betala dig ur fängelset.");
            return;
        }
        self.pay_player_to_free_parking_pot(self.current_player_index, JAIL_FINE);
        self.players[self.current_player_index].jailed = false;
        self.players[self.current_player_index].jail_turns = 0;
        let player_name = self.players[self.current_player_index].name.clone();
        self.bank_message = format!(
            "{player_name} betalar {JAIL_FINE} kr till Fri parkering-potten och lämnar fängelset."
        );
        self.push_event(format!(
            "{player_name} betalade sig ur fängelset. {JAIL_FINE} kr gick till Fri parkering-potten."
        ));
    }

    fn is_authorized_current_player(&mut self, player_name: &str) -> bool {
        let cleaned = clean_player_name(player_name);
        if cleaned.is_empty() {
            return true;
        }
        if self.players[self.current_player_index].name == cleaned {
            return true;
        }
        self.bank_message = format!(
            "Det är {}s tur, inte {}s.",
            self.players[self.current_player_index].name, cleaned
        );
        false
    }

    fn build_error(&self, player_index: usize, space_index: usize) -> Option<String> {
        let Some(space) = self.spaces.get(space_index) else {
            return Some("Välj en giltig gata att bygga på.".to_string());
        };
        if space.kind != "property" {
            return Some("Det går bara att bygga på gator, inte stationer eller verk.".to_string());
        }
        if self.owners.get(space_index).and_then(|owner| *owner) != Some(player_index) {
            return Some(format!("{} ägs inte av aktuell spelare.", space.name));
        }
        let Some(color) = space.color.as_deref() else {
            return Some("Gatan saknar färggrupp och kan inte byggas på.".to_string());
        };
        let group = self.color_group_indices(color);
        if group.is_empty()
            || !group
                .iter()
                .all(|index| self.owners[*index] == Some(player_index))
        {
            return Some(format!(
                "Du måste äga hela {}-gruppen innan du bygger.",
                color_group_label(color)
            ));
        }
        if group.iter().any(|index| self.mortgaged[*index]) {
            return Some("Lös alla inteckningar i färggruppen innan du bygger.".to_string());
        }
        let level = self.buildings[space_index];
        if level >= 5 {
            return Some(format!("{} har redan hotell.", space.name));
        }
        let min_level = group
            .iter()
            .map(|index| self.buildings[*index])
            .min()
            .unwrap_or(0);
        if level > min_level {
            return Some("Bygg jämnt i färggruppen innan nästa nivå.".to_string());
        }
        let cost = space.build_cost.unwrap_or(0);
        if cost <= 0 {
            return Some("Gatan saknar byggkostnad i TOML-reglerna.".to_string());
        }
        if self.players[player_index].cash < cost {
            return Some(format!(
                "{} har inte råd att bygga för {} kr.",
                self.players[player_index].name, cost
            ));
        }
        None
    }

    fn color_group_indices(&self, color: &str) -> Vec<usize> {
        self.spaces
            .iter()
            .filter(|space| space.kind == "property" && space.color.as_deref() == Some(color))
            .map(|space| space.index)
            .collect()
    }

    fn rent_for_space(&self, space_index: usize) -> i32 {
        if self.mortgaged[space_index] {
            return 0;
        }
        let space = &self.spaces[space_index];
        if space.kind == "station" {
            let Some(owner) = self.owners[space_index] else {
                return space.rent.unwrap_or(0);
            };
            let count = self
                .spaces
                .iter()
                .filter(|space| {
                    space.kind == "station"
                        && self.owners[space.index] == Some(owner)
                        && !self.mortgaged[space.index]
                })
                .count();
            return match count {
                0 => 0,
                1 => 250,
                2 => 500,
                3 => 1000,
                _ => 2000,
            };
        }
        if space.kind == "utility" {
            let Some(owner) = self.owners[space_index] else {
                return space.rent.unwrap_or(0);
            };
            let count = self
                .spaces
                .iter()
                .filter(|space| {
                    space.kind == "utility"
                        && self.owners[space.index] == Some(owner)
                        && !self.mortgaged[space.index]
                })
                .count();
            let dice_total = (self.dice[0] + self.dice[1]).max(1) as i32;
            return dice_total * if count >= 2 { 100 } else { 40 };
        }
        self.rent_for_level(space_index, self.buildings[space_index])
    }

    fn rent_for_level(&self, space_index: usize, level: u8) -> i32 {
        let space = &self.spaces[space_index];
        let level = level as usize;
        if level > 0 {
            return space
                .house_rents
                .get(level.saturating_sub(1))
                .copied()
                .unwrap_or_else(|| space.rent.unwrap_or(0));
        }
        space.rent.unwrap_or(0)
    }

    fn mortgage_value(&self, space_index: usize) -> i32 {
        self.spaces[space_index].price.unwrap_or(0) / 2
    }

    fn unmortgage_cost(&self, space_index: usize) -> i32 {
        let value = self.mortgage_value(space_index);
        value + value / 10
    }

    fn has_buildings_in_group(&self, space_index: usize) -> bool {
        let space = &self.spaces[space_index];
        if space.kind != "property" {
            return false;
        }
        let Some(color) = space.color.as_deref() else {
            return false;
        };
        self.color_group_indices(color)
            .iter()
            .any(|index| self.buildings[*index] > 0)
    }

    fn liquidation_value(&self, player_index: usize) -> i32 {
        self.spaces
            .iter()
            .filter(|space| self.owners[space.index] == Some(player_index))
            .map(|space| {
                let mortgage = if self.mortgaged[space.index] {
                    0
                } else {
                    self.mortgage_value(space.index)
                };
                let building = (self.buildings[space.index] as i32)
                    * self.spaces[space.index].build_cost.unwrap_or(0)
                    / 2;
                mortgage + building
            })
            .sum()
    }

    fn resolve_negative_cash(&mut self, player_index: usize) {
        if self.players[player_index].cash >= 0 || self.players[player_index].bankrupt {
            return;
        }
        let debt = -self.players[player_index].cash;
        if self.liquidation_value(player_index) >= debt {
            let name = self.players[player_index].name.clone();
            self.bank_message.push_str(&format!(
                " {name} ligger {debt} kr back och måste sälja byggnader eller inteckna fastigheter."
            ));
            self.push_event(format!("{name} ligger {debt} kr back."));
            return;
        }
        let name = self.players[player_index].name.clone();
        self.bank_message.push_str(&format!(
            " {name} ligger {debt} kr back och saknar täckning. Begär konkurs via banken eller försök få hjälp av andra spelare."
        ));
        self.push_event(format!(
            "{name} ligger {debt} kr back och måste begära konkurs eller förhandla."
        ));
    }

    fn declare_bankruptcy(&mut self, player_index: usize) {
        let name = self.players[player_index].name.clone();
        self.players[player_index].bankrupt = true;
        self.players[player_index].token = None;
        self.players[player_index].jailed = false;
        self.players[player_index].cash = 0;
        for index in 0..self.spaces.len() {
            if self.owners[index] == Some(player_index) {
                self.owners[index] = None;
                self.buildings[index] = 0;
                self.mortgaged[index] = false;
            }
        }
        self.pending_offer = None;
        self.bank_proposals.retain(|proposal| {
            proposal.requester_index != player_index && proposal.counterparty_index != player_index
        });
        self.push_event(format!(
            "{name} gick i konkurs. Fastigheterna återgår till banken."
        ));
        self.bank_message = format!("{name} är i konkurs och är ute ur spelet.");
        if self.current_player_index == player_index {
            self.advance_to_next_active_player();
        }
    }

    fn voluntary_exit_to_player(&mut self, player_index: usize, recipient_index: usize) -> String {
        if player_index == recipient_index {
            return "Du kan inte ge tillgångarna till dig själv när du går ur.".to_string();
        }
        if self.players[player_index].bankrupt {
            return "Spelaren är redan ute ur spelet.".to_string();
        }
        if self.players[recipient_index].bankrupt {
            return "Mottagaren är redan ute ur spelet.".to_string();
        }

        let player_name = self.players[player_index].name.clone();
        let recipient_name = self.players[recipient_index].name.clone();
        let mut transferred = Vec::new();
        for index in 0..self.spaces.len() {
            if self.owners[index] == Some(player_index) {
                self.owners[index] = Some(recipient_index);
                transferred.push(self.spaces[index].name.clone());
            }
        }
        let cash = self.players[player_index].cash.max(0);
        if cash > 0 {
            self.players[recipient_index].cash += cash;
        }
        self.players[player_index].bankrupt = true;
        self.players[player_index].token = None;
        self.players[player_index].jailed = false;
        self.players[player_index].cash = 0;
        self.pending_offer = self
            .pending_offer
            .take()
            .filter(|offer| offer.player_index != player_index);
        self.bank_proposals.retain(|proposal| {
            proposal.requester_index != player_index && proposal.counterparty_index != player_index
        });
        if self.current_player_index == player_index {
            self.advance_to_next_active_player();
        }

        let asset_text = if transferred.is_empty() {
            "inga fastigheter".to_string()
        } else {
            transferred.join(", ")
        };
        let cash_text = if cash > 0 {
            format!(" och {cash} kr")
        } else {
            String::new()
        };
        self.push_event(format!(
            "{player_name} gick ur spelet och gav {asset_text}{cash_text} till {recipient_name}."
        ));
        format!(
            "{player_name} går ur spelet. {recipient_name} tar över {asset_text}{cash_text}. Det här är frivilligt utträde, inte vanlig konkurs."
        )
    }

    fn advance_to_next_active_player(&mut self) {
        if self
            .players
            .iter()
            .filter(|player| !player.bankrupt)
            .count()
            <= 1
        {
            if let Some(winner) = self.players.iter().find(|player| !player.bankrupt) {
                self.bank_message = format!("{} vinner spelet!", winner.name);
            }
            return;
        }
        for _ in 0..self.players.len() {
            self.current_player_index = (self.current_player_index + 1) % self.players.len();
            if !self.players[self.current_player_index].bankrupt {
                return;
            }
        }
    }

    fn draw_and_apply_card(&mut self, deck: &str, player_name: &str) -> bool {
        let Some(card) = self.next_card(deck) else {
            self.bank_message
                .push_str(" Kortleken är tom, så inget händer.");
            return false;
        };

        self.drawn_card = Some(DrawnCard {
            deck: card.deck.clone(),
            id: card.id.clone(),
            title: card.title.clone(),
            text: card.text.clone(),
            icon: card.icon.clone(),
        });
        self.bank_message
            .push_str(&format!(" {}: {}", card.title, card.text));
        self.push_event(format!("{player_name} drog kort: {}", card.text));
        self.apply_card_effect(&card)
    }

    fn next_card(&mut self, deck: &str) -> Option<GameCard> {
        if deck == "chance" {
            if self.chance_cards.is_empty() {
                return None;
            }
            let card = self.chance_cards[self.next_chance % self.chance_cards.len()].clone();
            self.next_chance += 1;
            Some(card)
        } else {
            if self.community_cards.is_empty() {
                return None;
            }
            let card =
                self.community_cards[self.next_community % self.community_cards.len()].clone();
            self.next_community += 1;
            Some(card)
        }
    }

    fn apply_card_effect(&mut self, card: &GameCard) -> bool {
        let player_index = self.current_player_index;
        match card.effect.as_str() {
            "gain_money" => {
                let amount = card.amount.unwrap_or(0);
                self.players[player_index].cash += amount;
                self.push_event(format!(
                    "{} fick {} kr av banken.",
                    self.players[player_index].name, amount
                ));
                false
            }
            "pay_money" => {
                let amount = card.amount.unwrap_or(0);
                self.pay_player_to_free_parking_pot(player_index, amount);
                self.push_event(format!(
                    "{} betalade {} kr till Fri parkering-potten.",
                    self.players[player_index].name, amount
                ));
                self.bank_message.push_str(&format!(
                    " Pengarna går till Fri parkering-potten, som nu är {} kr.",
                    self.free_parking_pot
                ));
                self.resolve_negative_cash(player_index);
                false
            }
            "move_to" => {
                let target = card.target.unwrap_or(0);
                let old_position = self.players[player_index].position;
                if target <= old_position {
                    let amount = card.amount.unwrap_or(GO_SALARY);
                    self.players[player_index].cash += amount;
                    self.bank_message
                        .push_str(&format!(" Banken betalar {} kr för Gå.", amount));
                    self.push_event(format!(
                        "{} fick {} kr av banken för Gå.",
                        self.players[player_index].name, amount
                    ));
                }
                self.players[player_index].position = target;
                let target_name = self
                    .spaces
                    .get(target)
                    .map(|space| space.name.clone())
                    .unwrap_or_else(|| "Gå".to_string());
                self.push_event(format!(
                    "{} flyttade till {}.",
                    self.players[player_index].name, target_name
                ));
                false
            }
            "go_to_jail" => {
                let target = card.target.unwrap_or(10);
                self.players[player_index].position = target;
                self.players[player_index].jailed = true;
                self.players[player_index].jail_turns = 0;
                let target_name = self
                    .spaces
                    .get(target)
                    .map(|space| space.name.clone())
                    .unwrap_or_else(|| "Fängelse".to_string());
                self.push_event(format!(
                    "{} gick direkt till {}.",
                    self.players[player_index].name, target_name
                ));
                true
            }
            _ => false,
        }
    }

    fn finish_turn_after_roll(&mut self) {
        if self.dice[0] == self.dice[1] {
            self.bank_message
                .push_str(" Dubbel, samma spelare slår igen.");
        } else {
            self.advance_to_next_active_player();
        }
    }

    fn roll_die(&mut self) -> u8 {
        (self.next_random_u32() % 6 + 1) as u8
    }

    fn next_random_u32(&mut self) -> u32 {
        if self.rng_state == 0 {
            self.rng_state = seed_rng(self.current_player_index as u64 + 1);
        }
        let mut value = self.rng_state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.rng_state = value;
        (value.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    fn current_selector_index(&self) -> usize {
        if self.phase == Phase::TokenSelection {
            self.selection_order[self.selection_cursor]
        } else {
            self.current_player_index
        }
    }

    fn push_event(&mut self, event: String) {
        self.events.push(event);
        if self.events.len() > 8 {
            self.events.remove(0);
        }
    }

    fn queue_ai_action_if_needed(&mut self) -> bool {
        if self.stopped || self.ai_action_pending {
            return false;
        }
        let should_queue = match self.phase {
            Phase::TokenSelection => self
                .selection_order
                .get(self.selection_cursor)
                .and_then(|index| self.players.get(*index))
                .map(|player| player.controller == PlayerController::Ai && player.token.is_none())
                .unwrap_or(false),
            Phase::Play => {
                self.players
                    .get(self.current_player_index)
                    .map(|player| player.controller == PlayerController::Ai && !player.bankrupt)
                    .unwrap_or(false)
                    && self.auction.is_none()
            }
            Phase::Auction => self
                .auction
                .as_ref()
                .map(auction_seconds_left)
                .map(|seconds| seconds == 0)
                .unwrap_or(false),
        };
        if should_queue {
            self.ai_action_pending = true;
            let name = match self.phase {
                Phase::TokenSelection => self
                    .selection_order
                    .get(self.selection_cursor)
                    .and_then(|index| self.players.get(*index))
                    .map(|player| player.name.clone())
                    .unwrap_or_else(|| "AI".to_string()),
                _ => self
                    .players
                    .get(self.current_player_index)
                    .map(|player| player.name.clone())
                    .unwrap_or_else(|| "AI".to_string()),
            };
            self.ai_turn_source = "väntar".to_string();
            self.ai_turn_thought = format!("{name} tänker...");
            self.bank_message = format!("{name} tänker...");
        }
        should_queue
    }

    fn run_ai_action(&mut self) {
        if self.stopped {
            return;
        }
        match self.phase {
            Phase::TokenSelection => self.select_ai_token(),
            Phase::Play => self.run_ai_play_action(),
            Phase::Auction => self.finish_auction(),
        }
    }

    fn prepare_ai_turn_job(&mut self) -> Option<AiTurnJob> {
        if self.stopped || self.phase != Phase::Play {
            return None;
        }
        let player_index = self.current_player_index;
        if self
            .players
            .get(player_index)
            .map(|player| player.controller != PlayerController::Ai || player.bankrupt)
            .unwrap_or(true)
        {
            return None;
        }
        let settings = load_settings();
        let player_name = self.players[player_index].name.clone();
        let fallback_decision = self.fallback_ai_decision(player_index);
        let prompt = self.ai_turn_prompt(player_index, &settings, fallback_decision);
        self.ai_turn_source = "LLM".to_string();
        self.ai_turn_thought = format!("{player_name} frågar modellen efter nästa drag...");
        self.bank_message = format!("{player_name} tänker med AI...");
        Some(AiTurnJob {
            player_name,
            prompt,
            fallback_decision,
        })
    }

    fn apply_ai_turn_decision(&mut self, player_name: &str, decision: AiDecision, reason: &str) {
        if self.stopped || self.phase != Phase::Play {
            return;
        }
        let player_index = self.current_player_index;
        if self
            .players
            .get(player_index)
            .map(|player| {
                player.name != player_name
                    || player.controller != PlayerController::Ai
                    || player.bankrupt
            })
            .unwrap_or(true)
        {
            return;
        }
        let fallback = self.fallback_ai_decision(player_index);
        let requested = decision;
        let mut used_fallback = false;
        let chosen = if self.perform_ai_decision(player_index, decision) {
            decision
        } else {
            used_fallback = true;
            fallback
        };
        if chosen != decision {
            let _ = self.perform_ai_decision(player_index, fallback);
        }
        let status_is_fallback = self.bank_ai_status.starts_with("ai fallback");
        let source = if status_is_fallback || used_fallback {
            "Fallback"
        } else {
            "LLM"
        };
        self.ai_turn_source = source.to_string();
        self.ai_turn_thought = self.ai_turn_thought_text(
            player_name,
            source,
            requested,
            chosen,
            reason,
            used_fallback,
        );
    }

    fn perform_ai_decision(&mut self, player_index: usize, decision: AiDecision) -> bool {
        let player_name = self.players[player_index].name.clone();
        match decision {
            AiDecision::Buy => {
                let Some(offer) = &self.pending_offer else {
                    return false;
                };
                if offer.player_index != player_index {
                    return false;
                }
                self.buy_pending_property(&player_name);
                true
            }
            AiDecision::Decline => {
                let Some(offer) = &self.pending_offer else {
                    return false;
                };
                if offer.player_index != player_index {
                    return false;
                }
                self.decline_pending_property(&player_name);
                true
            }
            AiDecision::Roll => {
                if self.pending_offer.is_some() || self.players[player_index].cash < 0 {
                    return false;
                }
                self.roll_current_player(&player_name);
                true
            }
            AiDecision::PayJail => {
                if !self.players[player_index].jailed || self.players[player_index].cash < JAIL_FINE
                {
                    return false;
                }
                self.pay_jail_fine(&player_name);
                true
            }
            AiDecision::Liquidate => self.ai_liquidate_once(player_index),
            AiDecision::Bankrupt => {
                if self.players[player_index].cash >= 0 {
                    return false;
                }
                self.declare_bankruptcy(player_index);
                true
            }
            AiDecision::Build => self.ai_build_once(player_index),
            AiDecision::Trade => self.ai_propose_trade_once(player_index),
            AiDecision::AcceptProposal => {
                let Some(proposal_id) = self.pending_bank_proposal_for(player_index) else {
                    return false;
                };
                self.accept_bank_proposal(&player_name, proposal_id);
                true
            }
            AiDecision::DeclineProposal => {
                let Some(proposal_id) = self.pending_bank_proposal_for(player_index) else {
                    return false;
                };
                self.decline_bank_proposal(&player_name, proposal_id);
                true
            }
            AiDecision::Wait => false,
        }
    }

    fn ai_turn_thought_text(
        &self,
        player_name: &str,
        source: &str,
        requested: AiDecision,
        chosen: AiDecision,
        reason: &str,
        used_fallback: bool,
    ) -> String {
        let reason = clean_ai_reason(reason);
        let action_text = if used_fallback {
            format!(
                "{} föreslog {}, men servern körde {}.",
                source,
                requested.label_sv(),
                chosen.label_sv()
            )
        } else {
            format!("{} valde {}.", source, chosen.label_sv())
        };
        if reason.is_empty() {
            format!(
                "{player_name}: {action_text} Banken väger pengar, position och möjliga affärer innan turen går vidare."
            )
        } else if reason.contains("Ingen tydlig motivering") {
            format!(
                "{player_name}: {action_text} Modellen gav ett kort svar, så banken tolkar draget utifrån pengar, fastigheter och risk."
            )
        } else {
            format!("{player_name}: {action_text} {reason}")
        }
    }

    fn fallback_ai_decision(&self, player_index: usize) -> AiDecision {
        if let Some(offer) = &self.pending_offer {
            if offer.player_index == player_index {
                let price = self.spaces[offer.space_index].price.unwrap_or(0);
                return if self.players[player_index].cash >= price {
                    AiDecision::Buy
                } else {
                    AiDecision::Decline
                };
            }
        }
        if let Some(proposal) = self.awaiting_bank_proposal_for(player_index) {
            return if self.ai_should_accept_proposal(proposal, player_index) {
                AiDecision::AcceptProposal
            } else {
                AiDecision::DeclineProposal
            };
        }
        if self.players[player_index].cash < 0 {
            return if self.has_ai_liquidation_option(player_index) {
                AiDecision::Liquidate
            } else {
                AiDecision::Bankrupt
            };
        }
        if self.players[player_index].jailed
            && self.players[player_index].cash >= JAIL_FINE + 1000
            && self.players[player_index].jail_turns >= 1
        {
            return AiDecision::PayJail;
        }
        if self.ai_build_target(player_index).is_some() {
            return AiDecision::Build;
        }
        if self.ai_trade_candidate(player_index).is_some() {
            return AiDecision::Trade;
        }
        AiDecision::Roll
    }

    fn ai_allowed_actions(&self, player_index: usize) -> Vec<AiDecision> {
        if let Some(offer) = &self.pending_offer {
            if offer.player_index == player_index {
                return vec![AiDecision::Buy, AiDecision::Decline];
            }
        }
        if self.pending_bank_proposal_for(player_index).is_some() {
            return vec![AiDecision::AcceptProposal, AiDecision::DeclineProposal];
        }
        if self.players[player_index].cash < 0 {
            let mut actions = Vec::new();
            if self.has_ai_liquidation_option(player_index) {
                actions.push(AiDecision::Liquidate);
            }
            actions.push(AiDecision::Bankrupt);
            return actions;
        }
        if self.players[player_index].jailed {
            let mut actions = vec![AiDecision::Roll];
            if self.players[player_index].cash >= JAIL_FINE {
                actions.push(AiDecision::PayJail);
            }
            return actions;
        }
        let mut actions = Vec::new();
        if self.ai_build_target(player_index).is_some() {
            actions.push(AiDecision::Build);
        }
        if self.ai_trade_candidate(player_index).is_some() {
            actions.push(AiDecision::Trade);
        }
        actions.push(AiDecision::Roll);
        actions
    }

    fn ai_turn_prompt(
        &self,
        player_index: usize,
        settings: &Settings,
        fallback_decision: AiDecision,
    ) -> String {
        let player = &self.players[player_index];
        let space = &self.spaces[player.position];
        let allowed_actions = self
            .ai_allowed_actions(player_index)
            .into_iter()
            .map(AiDecision::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let personality = ai_personality_for_player(&player.name, &settings.ai_profiles_toml)
            .unwrap_or_else(|| "Ingen särskild personlighetsprofil. Spela rimligt.".to_string());
        let players = self
            .players
            .iter()
            .map(|other| {
                let other_space = self
                    .spaces
                    .get(other.position)
                    .map(|space| space.name.as_str())
                    .unwrap_or("okänd ruta");
                format!(
                    "- {}{}: {} kr, står på {}, äger {}",
                    other.name,
                    if other.controller == PlayerController::Ai {
                        " (AI)"
                    } else {
                        ""
                    },
                    other.cash,
                    other_space,
                    self.owned_assets_summary(
                        self.player_index_by_name(&other.name)
                            .unwrap_or(player_index)
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let pending = self
            .pending_offer
            .as_ref()
            .map(|offer| {
                format!(
                    "Pending köp: {} kan köpa {} för {} kr.",
                    self.players[offer.player_index].name,
                    self.spaces[offer.space_index].name,
                    self.spaces[offer.space_index].price.unwrap_or(0)
                )
            })
            .unwrap_or_else(|| "Inget pending köp.".to_string());
        format!(
            "{}\n\nPersonlighet för {}:\n{}\n\nAktuell AI-spelare:\n- Namn: {}\n- Pengar: {} kr\n- Ruta: {} ({})\n- Fängelse: {}, försök {}\n- Ägda tillgångar: {}\n\nSpelläget:\nFas: {}\nTur: {}\nBankmeddelande: {}\n{}\nSpelare:\n{}\nSenaste händelser:\n{}\n\nTillåtna handlingar: {}\nFallback om du är osäker: {}\nSvara endast JSON på en rad med action satt till en av de tillåtna handlingarna.",
            settings.ai_turn_prompt,
            player.name,
            personality,
            player.name,
            player.cash,
            space.name,
            space.kind,
            player.jailed,
            player.jail_turns,
            self.owned_assets_summary(player_index),
            self.phase.as_str(),
            self.players[self.current_selector_index()].name,
            self.bank_message,
            pending,
            players,
            self.events.join("\n"),
            allowed_actions,
            fallback_decision.as_str()
        )
    }

    fn select_ai_token(&mut self) {
        if self.phase != Phase::TokenSelection {
            return;
        }
        let Some(player_index) = self.selection_order.get(self.selection_cursor).copied() else {
            return;
        };
        if self.players[player_index].controller != PlayerController::Ai {
            return;
        }
        let Some(token) = self.first_available_token() else {
            self.bank_message = "AI kunde inte hitta en ledig pjäs.".to_string();
            return;
        };
        let name = self.players[player_index].name.clone();
        self.select_token(&token, &name);
    }

    fn run_ai_play_action(&mut self) {
        if self.phase != Phase::Play {
            return;
        }
        let player_index = self.current_player_index;
        if self.players[player_index].controller != PlayerController::Ai
            || self.players[player_index].bankrupt
        {
            return;
        }
        let player_name = self.players[player_index].name.clone();

        if let Some(offer) = &self.pending_offer {
            if offer.player_index == player_index {
                let price = self.spaces[offer.space_index].price.unwrap_or(0);
                if self.players[player_index].cash >= price {
                    self.buy_pending_property(&player_name);
                } else {
                    self.decline_pending_property(&player_name);
                }
            }
            return;
        }

        if self.players[player_index].cash < 0 {
            if self.ai_liquidate_once(player_index) {
                return;
            }
            self.declare_bankruptcy(player_index);
            return;
        }

        if self.players[player_index].jailed
            && self.players[player_index].cash >= JAIL_FINE + 1000
            && self.players[player_index].jail_turns >= 1
        {
            self.pay_jail_fine(&player_name);
            return;
        }

        self.roll_current_player(&player_name);
    }

    fn ai_liquidate_once(&mut self, player_index: usize) -> bool {
        if let Some(space_index) = self
            .spaces
            .iter()
            .find(|space| {
                self.owners[space.index] == Some(player_index) && self.buildings[space.index] > 0
            })
            .map(|space| space.index)
        {
            let name = self.players[player_index].name.clone();
            self.sell_building(space_index, &name);
            return true;
        }
        if let Some(space_index) = self
            .spaces
            .iter()
            .find(|space| {
                self.owners[space.index] == Some(player_index)
                    && !self.mortgaged[space.index]
                    && !self.has_buildings_in_group(space.index)
            })
            .map(|space| space.index)
        {
            let name = self.players[player_index].name.clone();
            self.mortgage_property(space_index, &name);
            return true;
        }
        false
    }

    fn ai_build_once(&mut self, player_index: usize) -> bool {
        let Some(space_index) = self.ai_build_target(player_index) else {
            return false;
        };
        let name = self.players[player_index].name.clone();
        self.build_property(space_index, &name);
        true
    }

    fn ai_build_target(&self, player_index: usize) -> Option<usize> {
        self.spaces
            .iter()
            .filter(|space| space.kind == "property")
            .filter(|space| self.owners[space.index] == Some(player_index))
            .filter(|space| self.build_error(player_index, space.index).is_none())
            .filter(|space| {
                let cost = space.build_cost.unwrap_or(0);
                self.players[player_index].cash >= cost + 1000
            })
            .max_by_key(|space| {
                let current = self.rent_for_space(space.index);
                let next_level = self.buildings[space.index].saturating_add(1);
                let next_rent = self.rent_for_level(space.index, next_level);
                (
                    next_rent - current,
                    std::cmp::Reverse(self.buildings[space.index]),
                )
            })
            .map(|space| space.index)
    }

    fn ai_propose_trade_once(&mut self, player_index: usize) -> bool {
        let Some(proposal) = self.ai_trade_candidate(player_index) else {
            return false;
        };
        let answer = self.push_bank_proposal(proposal);
        let name = self.players[player_index].name.clone();
        self.bank_message = format!("AI-handeln från {name}: {answer}");
        self.push_bank_message("Banken", &self.bank_message.clone(), true);
        true
    }

    fn ai_trade_candidate(&self, player_index: usize) -> Option<BankProposal> {
        if self
            .bank_proposals
            .iter()
            .any(|proposal| proposal.requester_index == player_index)
        {
            return None;
        }
        let rules = load_bank_rules();
        let mut best: Option<(i32, BankProposal)> = None;
        let colors = self
            .spaces
            .iter()
            .filter_map(|space| space.color.as_deref())
            .collect::<Vec<_>>();

        for color in colors {
            let group = self.color_group_indices(color);
            if group.len() < 2 {
                continue;
            }
            let owned = group
                .iter()
                .filter(|index| self.owners[**index] == Some(player_index))
                .count();
            if owned == 0 || owned == group.len() {
                continue;
            }
            for space_index in &group {
                let Some(owner) = self.owners[*space_index] else {
                    continue;
                };
                if owner == player_index || self.players[owner].bankrupt {
                    continue;
                }
                if self.has_buildings_in_group(*space_index) {
                    continue;
                }
                if self.bank_proposals.iter().any(|proposal| {
                    proposal.requester_index == player_index
                        && proposal.counterparty_index == owner
                        && proposal.spaces_from_counterparty.contains(space_index)
                }) {
                    continue;
                }
                let price = self.spaces[*space_index].price.unwrap_or(0);
                if price <= 0 {
                    continue;
                }
                let missing = group.len().saturating_sub(owned);
                let premium = if missing == 1 { 600 } else { 200 };
                let amount = ((price * 14) / 10 + premium).min(rules.trade_cash_limit);
                if amount <= 0 || self.players[player_index].cash < amount + 1000 {
                    continue;
                }
                let score = (owned as i32 * 10_000)
                    + if missing == 1 { 5_000 } else { 0 }
                    + self.rent_for_space(*space_index);
                let proposal = BankProposal {
                    id: 0,
                    kind: "ai_cash_offer".to_string(),
                    requester_index: player_index,
                    counterparty_index: owner,
                    awaiting_player_index: owner,
                    cash_from_requester: amount,
                    cash_from_counterparty: 0,
                    spaces_from_requester: Vec::new(),
                    spaces_from_counterparty: vec![*space_index],
                    note: format!("AI vill samla {}-gruppen.", color_group_label(color)),
                };
                if best
                    .as_ref()
                    .map(|(best_score, _)| score > *best_score)
                    .unwrap_or(true)
                {
                    best = Some((score, proposal));
                }
            }
        }
        best.map(|(_, proposal)| proposal)
    }

    fn pending_bank_proposal_for(&self, player_index: usize) -> Option<u64> {
        self.awaiting_bank_proposal_for(player_index)
            .map(|proposal| proposal.id)
    }

    fn awaiting_bank_proposal_for(&self, player_index: usize) -> Option<&BankProposal> {
        self.bank_proposals
            .iter()
            .find(|proposal| proposal.awaiting_player_index == player_index)
    }

    fn ai_should_accept_proposal(&self, proposal: &BankProposal, player_index: usize) -> bool {
        if self.proposal_breaks_complete_group(proposal, player_index) {
            return false;
        }
        self.proposal_value_for_player(proposal, player_index) >= 0
    }

    fn proposal_value_for_player(&self, proposal: &BankProposal, player_index: usize) -> i32 {
        let mut value = 0;
        if proposal.requester_index == player_index {
            value -= proposal.cash_from_requester;
            value += proposal.cash_from_counterparty;
            value -= self.spaces_value(&proposal.spaces_from_requester);
            value += self.spaces_value(&proposal.spaces_from_counterparty);
        } else if proposal.counterparty_index == player_index {
            value += proposal.cash_from_requester;
            value -= proposal.cash_from_counterparty;
            value += self.spaces_value(&proposal.spaces_from_requester);
            value -= self.spaces_value(&proposal.spaces_from_counterparty);
        }
        value
    }

    fn proposal_breaks_complete_group(&self, proposal: &BankProposal, player_index: usize) -> bool {
        let lost_spaces = if proposal.requester_index == player_index {
            &proposal.spaces_from_requester
        } else if proposal.counterparty_index == player_index {
            &proposal.spaces_from_counterparty
        } else {
            return false;
        };
        lost_spaces.iter().any(|space_index| {
            let Some(color) = self.spaces[*space_index].color.as_deref() else {
                return false;
            };
            let group = self.color_group_indices(color);
            !group.is_empty()
                && group
                    .iter()
                    .all(|index| self.owners[*index] == Some(player_index))
        })
    }

    fn spaces_value(&self, indexes: &[usize]) -> i32 {
        indexes
            .iter()
            .filter_map(|index| self.spaces.get(*index))
            .map(|space| {
                let base = space
                    .price
                    .unwrap_or_else(|| self.rent_for_space(space.index) * 10);
                let buildings = self.buildings[space.index] as i32 * space.build_cost.unwrap_or(0);
                base + buildings
            })
            .sum()
    }

    fn has_ai_liquidation_option(&self, player_index: usize) -> bool {
        self.spaces.iter().any(|space| {
            self.owners[space.index] == Some(player_index)
                && (self.buildings[space.index] > 0
                    || (!self.mortgaged[space.index] && !self.has_buildings_in_group(space.index)))
        })
    }

    fn owned_assets_summary(&self, player_index: usize) -> String {
        let assets = self
            .spaces
            .iter()
            .filter(|space| self.owners[space.index] == Some(player_index))
            .map(|space| {
                let building = match self.buildings[space.index] {
                    0 => "",
                    1..=4 => " med hus",
                    _ => " med hotell",
                };
                let mortgage = if self.mortgaged[space.index] {
                    ", intecknad"
                } else {
                    ""
                };
                format!("{}{}{}", space.name, building, mortgage)
            })
            .collect::<Vec<_>>();
        if assets.is_empty() {
            "inget".to_string()
        } else {
            assets.join(", ")
        }
    }

    fn first_available_token(&self) -> Option<String> {
        TOKEN_CHOICES
            .iter()
            .find(|(id, _)| {
                !self
                    .players
                    .iter()
                    .any(|player| player.token.as_deref() == Some(*id))
            })
            .map(|(id, _)| (*id).to_string())
    }

    fn accept_bank_proposal(&mut self, player_name: &str, proposal_id: u64) {
        let Some(player_index) = self.player_index_by_name(player_name) else {
            self.bank_message = "Banken hittar inte spelaren för det här förslaget.".to_string();
            return;
        };
        let Some(position) = self
            .bank_proposals
            .iter()
            .position(|proposal| proposal.id == proposal_id)
        else {
            self.bank_message = "Bankförslaget finns inte längre.".to_string();
            return;
        };
        if self.bank_proposals[position].awaiting_player_index != player_index {
            self.bank_message = "Det är inte din tur att svara på bankförslaget.".to_string();
            return;
        }

        let proposal = self.bank_proposals[position].clone();
        if let Some(error) = self.bank_proposal_error(&proposal) {
            self.bank_message = error;
            return;
        }

        self.apply_bank_proposal(&proposal);
        self.bank_proposals.remove(position);
        let summary = self.bank_proposal_summary(&proposal);
        self.bank_message = format!("Banken verkställde: {summary}");
        self.push_event(format!("Bankförslag accepterades: {summary}"));
        self.push_bank_message(
            "Banken",
            &format!("Accepterat och verkställt: {summary}"),
            true,
        );
    }

    fn decline_bank_proposal(&mut self, player_name: &str, proposal_id: u64) {
        let Some(player_index) = self.player_index_by_name(player_name) else {
            self.bank_message = "Banken hittar inte spelaren för det här förslaget.".to_string();
            return;
        };
        let Some(position) = self
            .bank_proposals
            .iter()
            .position(|proposal| proposal.id == proposal_id)
        else {
            self.bank_message = "Bankförslaget finns inte längre.".to_string();
            return;
        };
        if self.bank_proposals[position].awaiting_player_index != player_index {
            self.bank_message = "Det är inte din tur att svara på bankförslaget.".to_string();
            return;
        }
        let proposal = self.bank_proposals.remove(position);
        let name = self.players[player_index].name.clone();
        let summary = self.bank_proposal_summary(&proposal);
        self.bank_message = format!("{name} avböjde bankförslaget.");
        self.push_event(format!("{name} avböjde: {summary}"));
        self.push_bank_message("Banken", &format!("{name} avböjde: {summary}"), true);
    }

    fn counter_bank_proposal(&mut self, player_name: &str, proposal_id: u64, amount: i32) {
        let rules = load_bank_rules();
        let Some(player_index) = self.player_index_by_name(player_name) else {
            self.bank_message = "Banken hittar inte spelaren för motbudet.".to_string();
            return;
        };
        let Some(position) = self
            .bank_proposals
            .iter()
            .position(|proposal| proposal.id == proposal_id)
        else {
            self.bank_message = "Bankförslaget finns inte längre.".to_string();
            return;
        };
        if self.bank_proposals[position].awaiting_player_index != player_index {
            self.bank_message = "Det är inte din tur att lämna motbud.".to_string();
            return;
        }
        if amount <= 0 || amount > rules.trade_cash_limit {
            self.bank_message = format!(
                "Motbudet måste vara mellan 1 och {} kr.",
                rules.trade_cash_limit
            );
            return;
        }

        let other = if player_index == self.bank_proposals[position].requester_index {
            self.bank_proposals[position].counterparty_index
        } else {
            self.bank_proposals[position].requester_index
        };

        if self.bank_proposals[position].kind == "player_loan" {
            self.bank_proposals[position].cash_from_counterparty = amount;
        } else {
            self.bank_proposals[position].cash_from_requester = amount;
        }
        self.bank_proposals[position].awaiting_player_index = other;
        self.bank_proposals[position].note = format!(
            "Motbud från {}: {} kr.",
            self.players[player_index].name, amount
        );
        let summary = self.bank_proposal_summary(&self.bank_proposals[position]);
        self.bank_message = format!("Banken skickade motbudet vidare: {summary}");
        self.push_event(format!("Motbud via banken: {summary}"));
    }

    fn bank_proposal_error(&self, proposal: &BankProposal) -> Option<String> {
        if proposal.requester_index >= self.players.len()
            || proposal.counterparty_index >= self.players.len()
            || proposal.awaiting_player_index >= self.players.len()
        {
            return Some("Bankförslaget pekar på en okänd spelare.".to_string());
        }
        if self.players[proposal.requester_index].cash < proposal.cash_from_requester {
            return Some(format!(
                "{} har inte {} kr.",
                self.players[proposal.requester_index].name, proposal.cash_from_requester
            ));
        }
        if self.players[proposal.counterparty_index].cash < proposal.cash_from_counterparty {
            return Some(format!(
                "{} har inte {} kr.",
                self.players[proposal.counterparty_index].name, proposal.cash_from_counterparty
            ));
        }
        for space_index in &proposal.spaces_from_requester {
            if self.owners.get(*space_index).copied().flatten() != Some(proposal.requester_index) {
                return Some(format!(
                    "{} äger inte {} längre.",
                    self.players[proposal.requester_index].name,
                    self.spaces
                        .get(*space_index)
                        .map(|space| space.name.as_str())
                        .unwrap_or("rutan")
                ));
            }
        }
        for space_index in &proposal.spaces_from_counterparty {
            if self.owners.get(*space_index).copied().flatten() != Some(proposal.counterparty_index)
            {
                return Some(format!(
                    "{} äger inte {} längre.",
                    self.players[proposal.counterparty_index].name,
                    self.spaces
                        .get(*space_index)
                        .map(|space| space.name.as_str())
                        .unwrap_or("rutan")
                ));
            }
        }
        None
    }

    fn apply_bank_proposal(&mut self, proposal: &BankProposal) {
        if proposal.cash_from_requester > 0 {
            self.players[proposal.requester_index].cash -= proposal.cash_from_requester;
            self.players[proposal.counterparty_index].cash += proposal.cash_from_requester;
        }
        if proposal.cash_from_counterparty > 0 {
            self.players[proposal.counterparty_index].cash -= proposal.cash_from_counterparty;
            self.players[proposal.requester_index].cash += proposal.cash_from_counterparty;
        }
        for space_index in &proposal.spaces_from_requester {
            if let Some(owner) = self.owners.get_mut(*space_index) {
                *owner = Some(proposal.counterparty_index);
            }
        }
        for space_index in &proposal.spaces_from_counterparty {
            if let Some(owner) = self.owners.get_mut(*space_index) {
                *owner = Some(proposal.requester_index);
            }
        }
        self.resolve_negative_cash(proposal.requester_index);
        self.resolve_negative_cash(proposal.counterparty_index);
    }

    fn bank_proposal_summary(&self, proposal: &BankProposal) -> String {
        let requester = &self.players[proposal.requester_index].name;
        let counterparty = &self.players[proposal.counterparty_index].name;
        let mut parts = Vec::new();
        if proposal.cash_from_requester > 0 {
            parts.push(format!(
                "{requester} betalar {} kr",
                proposal.cash_from_requester
            ));
        }
        if proposal.cash_from_counterparty > 0 {
            parts.push(format!(
                "{counterparty} betalar {} kr",
                proposal.cash_from_counterparty
            ));
        }
        let requester_spaces = self.space_names(&proposal.spaces_from_requester);
        if !requester_spaces.is_empty() {
            parts.push(format!("{requester} ger {requester_spaces}"));
        }
        let counterparty_spaces = self.space_names(&proposal.spaces_from_counterparty);
        if !counterparty_spaces.is_empty() {
            parts.push(format!("{counterparty} ger {counterparty_spaces}"));
        }
        if parts.is_empty() {
            format!("{requester} och {counterparty} har ett tomt förslag")
        } else {
            parts.join(" · ")
        }
    }

    fn space_names(&self, indexes: &[usize]) -> String {
        indexes
            .iter()
            .filter_map(|index| self.spaces.get(*index))
            .map(|space| space.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn push_bank_proposal(&mut self, mut proposal: BankProposal) -> String {
        proposal.id = self.next_bank_proposal_id;
        self.next_bank_proposal_id += 1;
        let summary = self.bank_proposal_summary(&proposal);
        let awaiting = self.players[proposal.awaiting_player_index].name.clone();
        self.bank_proposals.push(proposal);
        self.push_event(format!(
            "Banken skickade förslag till {awaiting}: {summary}"
        ));
        format!("Jag har skickat ett förslag till {awaiting}: {summary}.")
    }

    fn ask_bank(&mut self, player_name: &str, message: &str) -> Option<BankAsyncJob> {
        let player_name = clean_player_name(player_name);
        let message = clean_chat_message(message);
        if player_name.is_empty() {
            self.push_bank_message(
                "Banken",
                "Skriv ditt namn på mobilen först, så vet jag vem jag pratar med.",
                true,
            );
            return None;
        }
        if message.is_empty() {
            self.push_bank_message("Banken", "Skriv en fråga till banken först.", true);
            return None;
        }

        self.push_bank_message(&player_name, &message, false);
        if self.stopped {
            let answer = "Spelet är stoppat av admin. Jag kan inte göra lån, byten, konkurs eller andra spelhandlingar förrän ett nytt spel startas.";
            self.bank_message = format!("Banken till {player_name}: {answer}");
            self.push_bank_message("Banken", answer, true);
            return None;
        }
        if let Some(answer) = self.try_bank_broker_action(&player_name, &message) {
            self.bank_message = format!("Banken till {player_name}: {answer}");
            self.push_bank_message("Banken", &answer, true);
            return None;
        }

        let prompt = self.bank_prompt(&player_name, &message, &load_settings().preprompt);
        let fallback_answer = self.mock_bank_answer(&player_name, &message);
        let thinking = "Jag tänker en stund. Spelet fortsätter under tiden.";
        self.bank_message = format!("Banken till {player_name}: {thinking}");
        self.push_bank_message("Banken", thinking, true);
        Some(BankAsyncJob {
            player_name,
            prompt,
            fallback_answer,
        })
    }

    fn finish_async_bank_answer(&mut self, player_name: &str, answer: &str, status: &str) {
        self.bank_ai_status = status.to_string();
        self.bank_message = format!("Banken till {player_name}: {answer}");
        self.push_bank_message("Banken", answer, true);
    }

    fn try_bank_broker_action(&mut self, player_name: &str, message: &str) -> Option<String> {
        let requester_index = self.player_index_by_name(player_name)?;
        let amount = extract_first_amount(message).unwrap_or(0);
        let is_emergency_loan = looks_like_emergency_loan(message);
        if is_emergency_loan {
            return Some(self.handle_emergency_loan_request(requester_index, message));
        }
        if looks_like_exit_or_bankruptcy(message) {
            return Some(self.handle_exit_or_bankruptcy_request(requester_index, message));
        }

        let wants_offer = contains_any_word(message, &["bud", "buda", "köp", "köpa", "köper"]);
        let wants_trade = contains_any_word(message, &["byt", "byte", "byta"]);
        let wants_loan = contains_any_word(message, &["låna", "lån"]);
        let wants_donation = contains_any_word(message, &["donera", "skänk", "ge"]) && amount > 0;
        let wants_real_action = wants_offer || wants_trade || wants_loan || wants_donation;
        if !wants_real_action {
            return None;
        }

        if self.phase != Phase::Play || self.current_player_index != requester_index {
            return Some(format!(
                "Du kan chatta med mig när som helst, men riktiga bud, byten, lån och donationer gör vi på din tur. Nu är det {}s tur.",
                self.players[self.current_selector_index()].name
            ));
        }

        let rules = load_bank_rules();
        if wants_donation {
            return Some(self.create_donation_proposal(requester_index, message, amount, &rules));
        }
        if wants_loan && amount > 0 {
            return Some(self.create_player_loan_proposal(
                requester_index,
                message,
                amount,
                &rules,
            ));
        }
        if wants_trade {
            return Some(self.create_trade_proposal(requester_index, message, amount, &rules));
        }
        if wants_offer {
            return Some(self.create_cash_offer_proposal(requester_index, message, amount, &rules));
        }

        Some("Jag förstår att du vill göra något bankmässigt, men jag behöver spelare, belopp och/eller fastighetsnamn tydligare.".to_string())
    }

    fn handle_exit_or_bankruptcy_request(&mut self, player_index: usize, message: &str) -> String {
        if let Some(recipient_index) = self.find_other_player_in_text(message, player_index) {
            return self.voluntary_exit_to_player(player_index, recipient_index);
        }
        if contains_any_word(message, &["konkurs"]) {
            let player_name = self.players[player_index].name.clone();
            self.declare_bankruptcy(player_index);
            return format!(
                "{player_name} begär konkurs. Alla fastigheter återgår till banken och blir tillgängliga för öppet köp."
            );
        }
        "Vill du gå ur frivilligt och ge bort tillgångar behöver du skriva mottagarens namn, till exempel: jag går ur och ger allt till Noel.".to_string()
    }

    fn handle_emergency_loan_request(&mut self, player_index: usize, message: &str) -> String {
        let rules = load_bank_rules();
        let requested = extract_first_amount(message).unwrap_or(rules.emergency_loan_limit);
        let amount = requested.clamp(1, rules.emergency_loan_limit);
        let player_name = self.players[player_index].name.clone();
        let nags = self
            .bank_chat
            .iter()
            .filter(|chat| {
                !chat.from_bank
                    && chat.speaker == player_name
                    && looks_like_emergency_loan(&chat.text)
            })
            .count();
        if nags < rules.emergency_loan_required_requests {
            let remaining = rules.emergency_loan_required_requests.saturating_sub(nags);
            return format!(
                "Nödlån är hårt hållna. Jag hör dig, {player_name}, men tjata {} gång{} till om du verkligen behöver upp till {} kr.",
                remaining,
                if remaining == 1 { "" } else { "er" },
                rules.emergency_loan_limit
            );
        }
        self.players[player_index].cash += amount;
        self.push_event(format!(
            "{player_name} fick nödlån från banken: {amount} kr."
        ));
        format!(
            "Okej. Banken ger {player_name} ett nödlån på {amount} kr. Det här är en husregel och bör användas sparsamt."
        )
    }

    fn create_donation_proposal(
        &mut self,
        requester_index: usize,
        message: &str,
        amount: i32,
        rules: &BankRules,
    ) -> String {
        if amount > rules.donation_limit {
            return format!(
                "Donationer är begränsade till {} kr i bankreglerna.",
                rules.donation_limit
            );
        }
        let Some(counterparty_index) = self.find_other_player_in_text(message, requester_index)
        else {
            return "Vem vill du ge pengar till? Skriv spelarens namn tydligt.".to_string();
        };
        self.push_bank_proposal(BankProposal {
            id: 0,
            kind: "donation".to_string(),
            requester_index,
            counterparty_index,
            awaiting_player_index: counterparty_index,
            cash_from_requester: amount,
            cash_from_counterparty: 0,
            spaces_from_requester: Vec::new(),
            spaces_from_counterparty: Vec::new(),
            note: "Donation via banken.".to_string(),
        })
    }

    fn create_player_loan_proposal(
        &mut self,
        requester_index: usize,
        message: &str,
        amount: i32,
        rules: &BankRules,
    ) -> String {
        if amount > rules.player_loan_limit {
            return format!(
                "Spelarlån är begränsade till {} kr i bankreglerna.",
                rules.player_loan_limit
            );
        }
        let Some(counterparty_index) = self.find_other_player_in_text(message, requester_index)
        else {
            return "Vem vill du låna pengar av? Skriv spelarens namn tydligt.".to_string();
        };
        self.push_bank_proposal(BankProposal {
            id: 0,
            kind: "player_loan".to_string(),
            requester_index,
            counterparty_index,
            awaiting_player_index: counterparty_index,
            cash_from_requester: 0,
            cash_from_counterparty: amount,
            spaces_from_requester: Vec::new(),
            spaces_from_counterparty: Vec::new(),
            note: "Lån mellan spelare via banken.".to_string(),
        })
    }

    fn create_cash_offer_proposal(
        &mut self,
        requester_index: usize,
        message: &str,
        amount: i32,
        rules: &BankRules,
    ) -> String {
        if amount > rules.trade_cash_limit {
            return format!(
                "Bud är begränsade till {} kr i bankreglerna.",
                rules.trade_cash_limit
            );
        }
        let Some(space_index) =
            self.find_owned_space_in_text(message, Some(requester_index), false)
        else {
            return "Vilken fastighet vill du lägga bud på? Skriv gatunamnet tydligare."
                .to_string();
        };
        let Some(counterparty_index) = self.owners[space_index] else {
            return "Den fastigheten ägs inte av någon spelare just nu.".to_string();
        };
        let offer_amount = if amount > 0 {
            amount
        } else {
            self.spaces[space_index].price.unwrap_or(1000)
        };
        if offer_amount > rules.trade_cash_limit {
            return format!(
                "Bud är begränsade till {} kr i bankreglerna.",
                rules.trade_cash_limit
            );
        }
        self.push_bank_proposal(BankProposal {
            id: 0,
            kind: "cash_offer".to_string(),
            requester_index,
            counterparty_index,
            awaiting_player_index: counterparty_index,
            cash_from_requester: offer_amount,
            cash_from_counterparty: 0,
            spaces_from_requester: Vec::new(),
            spaces_from_counterparty: vec![space_index],
            note: "Bud på fastighet via banken.".to_string(),
        })
    }

    fn create_trade_proposal(
        &mut self,
        requester_index: usize,
        message: &str,
        amount: i32,
        rules: &BankRules,
    ) -> String {
        if amount > rules.trade_cash_limit {
            return format!(
                "Kontantdel i byte är begränsad till {} kr i bankreglerna.",
                rules.trade_cash_limit
            );
        }
        let Some(counterparty_space) =
            self.find_owned_space_in_text(message, Some(requester_index), false)
        else {
            return "Vilken fastighet vill du få i bytet? Skriv namnet tydligare.".to_string();
        };
        let Some(counterparty_index) = self.owners[counterparty_space] else {
            return "Fastigheten du vill få ägs inte av någon spelare.".to_string();
        };
        let requester_space = self.find_owned_space_in_text(message, Some(requester_index), true);
        self.push_bank_proposal(BankProposal {
            id: 0,
            kind: "property_trade".to_string(),
            requester_index,
            counterparty_index,
            awaiting_player_index: counterparty_index,
            cash_from_requester: amount.max(0),
            cash_from_counterparty: 0,
            spaces_from_requester: requester_space.into_iter().collect(),
            spaces_from_counterparty: vec![counterparty_space],
            note: "Byte via banken.".to_string(),
        })
    }

    fn player_index_by_name(&self, player_name: &str) -> Option<usize> {
        let target = normalize_lookup(player_name);
        self.players
            .iter()
            .position(|player| normalize_lookup(&player.name) == target)
    }

    fn find_other_player_in_text(&self, text: &str, requester_index: usize) -> Option<usize> {
        let normalized = normalize_lookup(text);
        self.players
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != requester_index)
            .filter_map(|(index, player)| {
                let player_key = normalize_lookup(&player.name);
                let numbered_key = format!("spelare{}", index + 1);
                if normalized.contains(&player_key) || normalized.contains(&numbered_key) {
                    Some((index, player_key.len().max(numbered_key.len())))
                } else {
                    None
                }
            })
            .max_by_key(|(_, score)| *score)
            .map(|(index, _)| index)
    }

    fn find_owned_space_in_text(
        &self,
        text: &str,
        requester_index: Option<usize>,
        owned_by_requester: bool,
    ) -> Option<usize> {
        let normalized = normalize_lookup(text);
        self.spaces
            .iter()
            .filter(|space| matches!(space.kind.as_str(), "property" | "station" | "utility"))
            .filter(|space| {
                let owner = self.owners[space.index];
                match (requester_index, owned_by_requester) {
                    (Some(requester), true) => owner == Some(requester),
                    (Some(requester), false) => owner.is_some() && owner != Some(requester),
                    (None, _) => owner.is_some(),
                }
            })
            .filter_map(|space| {
                let key = normalize_lookup(&space.name);
                if normalized.contains(&key) {
                    Some((space.index, key.len()))
                } else {
                    None
                }
            })
            .max_by_key(|(_, score)| *score)
            .map(|(index, _)| index)
    }

    fn admin_bank_message(&mut self, message: &str) {
        let message = clean_chat_message(message);
        if message.is_empty() {
            self.push_bank_message(
                "Banken",
                "Admin försökte skicka ett tomt bankmeddelande.",
                true,
            );
            return;
        }
        self.bank_message = format!("Banken: {message}");
        self.push_bank_message("Banken", &message, true);
        self.push_event(format!("Banken sade: {message}"));
    }

    fn push_bank_message(&mut self, speaker: &str, text: &str, from_bank: bool) {
        self.bank_chat.push(BankChatMessage {
            speaker: speaker.to_string(),
            text: text.to_string(),
            from_bank,
        });
        if self.bank_chat.len() > 24 {
            self.bank_chat.remove(0);
        }
    }

    fn bank_prompt(&self, player_name: &str, message: &str, preprompt: &str) -> String {
        let player_context = self
            .players
            .iter()
            .map(|player| {
                let space = self
                    .spaces
                    .get(player.position)
                    .map(|space| space.name.as_str())
                    .unwrap_or("okänd ruta");
                format!(
                    "- {}: {} kr, står på {}, pjäs {}, fängelse {}, konkurs {}",
                    player.name,
                    player.cash,
                    space,
                    player.token.as_deref().unwrap_or("ingen"),
                    player.jailed,
                    player.bankrupt
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let latest_events = self.events.join("\n");
        format!(
            "{preprompt}\n\nAktuell spelstatus:\nTur: {}\nFas: {}\nBankmeddelande: {}\nSpelare:\n{}\nSenaste händelser:\n{}\n\nSpelaren {player_name} frågar: {message}\nSvara kort på svenska som bank/spelledare. Ge bara hjälp om spelet och reglerna.",
            self.players[self.current_selector_index()].name,
            self.phase.as_str(),
            self.bank_message,
            player_context,
            latest_events
        )
    }

    fn mock_bank_answer(&self, player_name: &str, message: &str) -> String {
        let Some(player_index) = self
            .players
            .iter()
            .position(|player| player.name == player_name)
        else {
            return "Jag hittar inte din spelare ännu. Välj pjäs först, sedan kan jag hjälpa dig med läget.".to_string();
        };
        let player = &self.players[player_index];
        let space = &self.spaces[player.position];
        let lower = message.to_lowercase();
        let owned = self
            .spaces
            .iter()
            .filter(|space| self.owners[space.index] == Some(player_index))
            .map(|space| space.name.clone())
            .collect::<Vec<_>>();

        if let Some(offer) = &self.pending_offer {
            if offer.player_index == player_index {
                let offer_space = &self.spaces[offer.space_index];
                return format!(
                    "Du kan köpa {} för {} kr. Du har {} kr. Om du avstår startar auktion.",
                    offer_space.name,
                    offer_space.price.unwrap_or(0),
                    player.cash
                );
            }
        }
        if let Some(auction) = &self.auction {
            return format!(
                "Auktion pågår om {}. Högsta bud är {} kr{}.",
                self.spaces[auction.space_index].name,
                auction.highest_bid,
                auction
                    .highest_bidder
                    .map(|index| format!(" från {}", self.players[index].name))
                    .unwrap_or_default()
            );
        }
        if lower.contains("tur") || lower.contains("kasta") || lower.contains("slå") {
            if self.phase == Phase::TokenSelection {
                return format!(
                    "Vi är fortfarande i pjäsval. {} ska välja pjäs nu.",
                    self.players[self.current_selector_index()].name
                );
            }
            if self.players[self.current_player_index].name == player_name {
                return "Ja, det är din tur. Kasta tärningen när du är redo.".to_string();
            }
            return format!(
                "Inte riktigt än. Det är {}s tur.",
                self.players[self.current_player_index].name
            );
        }
        if lower.contains("var") || lower.contains("plats") || lower.contains("står") {
            return format!("Du står på {} och har {} kr.", space.name, player.cash);
        }
        if lower.contains("peng") || lower.contains("råd") || lower.contains("cash") {
            return format!(
                "Du har {} kr. Du äger {}.",
                player.cash,
                list_or_none(&owned)
            );
        }
        if lower.contains("hyra") {
            let owner_text = self.owners[space.index]
                .map(|owner| format!(" och ägs av {}", self.players[owner].name))
                .unwrap_or_else(|| " och är inte ägd av någon".to_string());
            return format!(
                "{} har aktuell hyra {} kr{}.",
                space.name,
                self.rent_for_space(space.index),
                owner_text
            );
        }
        if lower.contains("bygg") || lower.contains("hus") || lower.contains("hotell") {
            let buildable = self.buildable_properties_for(player_index);
            if buildable.is_empty() {
                return "Du har inget byggbart just nu. Du behöver äga en hel färggrupp och bygga jämnt.".to_string();
            }
            return format!(
                "Du kan bygga på {} när det är din tur och ingen köpfråga eller auktion pågår.",
                list_or_none(&buildable)
            );
        }
        if lower.contains("äger") || lower.contains("mina") {
            return format!("Du äger {}.", list_or_none(&owned));
        }

        format!(
            "Jag ser dig på {} med {} kr. Senaste läget: {}",
            space.name, player.cash, self.bank_message
        )
    }

    fn buildable_properties_for(&self, player_index: usize) -> Vec<String> {
        self.spaces
            .iter()
            .filter(|space| {
                space.kind == "property"
                    && self.owners[space.index] == Some(player_index)
                    && self.build_error(player_index, space.index).is_none()
            })
            .map(|space| space.name.clone())
            .collect()
    }

    fn to_json(&self) -> String {
        let current = &self.players[self.current_selector_index()].name;
        let players = self
            .players
            .iter()
            .map(|player| {
                format!(
                    "{{\"name\":\"{}\",\"controller\":\"{}\",\"cash\":{},\"position\":{},\"token\":{},\"jailed\":{},\"jailTurns\":{},\"bankrupt\":{}}}",
                    escape_json(&player.name),
                    player.controller.as_str(),
                    player.cash,
                    player.position,
                    optional_json_string(player.token.as_deref()),
                    player.jailed,
                    player.jail_turns,
                    player.bankrupt
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
                let rent_tiers = space
                    .house_rents
                    .iter()
                    .map(|rent| rent.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"index\":{},\"kind\":\"{}\",\"name\":\"{}\",\"color\":{},\"price\":{},\"rent\":{},\"currentRent\":{},\"buildCost\":{},\"buildings\":{},\"mortgaged\":{},\"mortgageValue\":{},\"unmortgageCost\":{},\"rentTiers\":[{}],\"amount\":{},\"target\":{},\"owner\":{},\"cardTitle\":{},\"cardText\":{},\"cardIcon\":{}}}",
                    space.index,
                    escape_json(&space.kind),
                    escape_json(&space.name),
                    optional_json_string(space.color.as_deref()),
                    optional_json_number(space.price),
                    optional_json_number(space.rent),
                    self.rent_for_space(space.index),
                    optional_json_number(space.build_cost),
                    self.buildings[space.index],
                    self.mortgaged[space.index],
                    self.mortgage_value(space.index),
                    self.unmortgage_cost(space.index),
                    rent_tiers,
                    optional_json_number(space.amount),
                    optional_json_usize(space.target),
                    owner,
                    optional_json_string(space.card_title.as_deref()),
                    optional_json_string(space.card_text.as_deref()),
                    optional_json_string(space.card_icon.as_deref())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let events = self
            .events
            .iter()
            .map(|event| format!("\"{}\"", escape_json(event)))
            .collect::<Vec<_>>()
            .join(",");
        let bank_chat = self
            .bank_chat
            .iter()
            .map(|message| {
                format!(
                    "{{\"speaker\":\"{}\",\"text\":\"{}\",\"fromBank\":{}}}",
                    escape_json(&message.speaker),
                    escape_json(&message.text),
                    message.from_bank
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
            "{{\"roomCode\":\"{}\",\"phase\":\"{}\",\"currentPlayer\":\"{}\",\"bankMessage\":\"{}\",\"aiTurnSource\":\"{}\",\"aiTurnThought\":\"{}\",\"stopped\":{},\"gameOver\":{},\"dice\":[{},{}],\"freeParkingPot\":{},\"pendingOffer\":{},\"auction\":{},\"drawnCard\":{},\"buildableProperties\":[{}],\"assetActions\":[{}],\"bankProposals\":[{}],\"players\":[{}],\"tokenChoices\":[{}],\"spaces\":[{}],\"events\":[{}],\"bankChat\":[{}]}}",
            escape_json(&self.room_code),
            self.phase.as_str(),
            escape_json(current),
            escape_json(&self.bank_message),
            escape_json(&self.ai_turn_source),
            escape_json(&self.ai_turn_thought),
            self.stopped,
            self.game_over_json(),
            self.dice[0],
            self.dice[1],
            self.free_parking_pot,
            self.pending_offer_json(),
            self.auction_json(),
            self.drawn_card_json(),
            self.buildable_properties_json(),
            self.asset_actions_json(),
            self.bank_proposals_json(),
            players,
            token_choices,
            spaces,
            events,
            bank_chat
        )
    }

    fn game_over_json(&self) -> String {
        let active = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, player)| !player.bankrupt)
            .collect::<Vec<_>>();
        if active.len() != 1 || self.phase == Phase::TokenSelection {
            return "null".to_string();
        }
        let (winner_index, winner) = active[0];
        let property_count = self
            .owners
            .iter()
            .filter(|owner| **owner == Some(winner_index))
            .count();
        let building_count: u32 = self
            .buildings
            .iter()
            .enumerate()
            .filter(|(index, _)| self.owners[*index] == Some(winner_index))
            .map(|(_, level)| u32::from(*level).min(4))
            .sum();
        let hotel_count = self
            .buildings
            .iter()
            .enumerate()
            .filter(|(index, level)| self.owners[*index] == Some(winner_index) && **level >= 5)
            .count();
        let asset_value = self.player_asset_value(winner_index);
        let net_worth = winner.cash + asset_value;
        let summary = format!(
            "{} vann med {} kr i kontanter och ett uppskattat värde på {} kr.",
            winner.name, winner.cash, net_worth
        );
        let player_rows = self
            .players
            .iter()
            .enumerate()
            .map(|(index, player)| {
                let properties = self.owners.iter().filter(|owner| **owner == Some(index)).count();
                format!(
                    "{{\"name\":\"{}\",\"cash\":{},\"bankrupt\":{},\"properties\":{},\"assetValue\":{},\"netWorth\":{}}}",
                    escape_json(&player.name),
                    player.cash,
                    player.bankrupt,
                    properties,
                    self.player_asset_value(index),
                    player.cash + self.player_asset_value(index)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let story = self
            .events
            .iter()
            .rev()
            .filter(|event| {
                let lower = event.to_lowercase();
                lower.contains("konkurs")
                    || lower.contains("gick ur")
                    || lower.contains("gav")
                    || lower.contains("vinner")
            })
            .take(5)
            .cloned()
            .collect::<Vec<_>>();
        let story = story
            .iter()
            .rev()
            .map(|event| format!("\"{}\"", escape_json(event)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"winner\":\"{}\",\"winnerCash\":{},\"winnerNetWorth\":{},\"propertyCount\":{},\"buildingCount\":{},\"hotelCount\":{},\"summary\":\"{}\",\"story\":[{}],\"players\":[{}]}}",
            escape_json(&winner.name),
            winner.cash,
            net_worth,
            property_count,
            building_count,
            hotel_count,
            escape_json(&summary),
            story,
            player_rows
        )
    }

    fn player_asset_value(&self, player_index: usize) -> i32 {
        self.spaces
            .iter()
            .filter(|space| self.owners[space.index] == Some(player_index))
            .map(|space| {
                let property = space.price.unwrap_or(0);
                let buildings =
                    (self.buildings[space.index] as i32) * space.build_cost.unwrap_or(0);
                property + buildings
            })
            .sum()
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
        let seconds_left = auction_seconds_left(auction);
        let can_finish = seconds_left == 0;
        format!(
            "{{\"spaceIndex\":{},\"spaceName\":\"{}\",\"highestBid\":{},\"highestBidder\":{},\"nextBid\":{},\"seller\":\"{}\",\"secondsLeft\":{},\"canFinish\":{}}}",
            auction.space_index,
            escape_json(&self.spaces[auction.space_index].name),
            auction.highest_bid,
            highest_bidder,
            next_bid,
            escape_json(&self.players[auction.seller_turn_index].name),
            seconds_left,
            can_finish
        )
    }

    fn buildable_properties_json(&self) -> String {
        if self.phase != Phase::Play || self.pending_offer.is_some() || self.auction.is_some() {
            return String::new();
        }

        let player_index = self.current_player_index;
        self.spaces
            .iter()
            .filter(|space| space.kind == "property" && self.owners[space.index] == Some(player_index))
            .map(|space| {
                let level = self.buildings[space.index];
                let next_level = if level < 5 { level + 1 } else { level };
                let can_build = self.build_error(player_index, space.index).is_none();
                let cost = space.build_cost.unwrap_or(0);
                let rent_after = if level < 5 {
                    space
                        .house_rents
                        .get(level as usize)
                        .copied()
                        .unwrap_or_else(|| self.rent_for_space(space.index))
                } else {
                    self.rent_for_space(space.index)
                };
                format!(
                    "{{\"spaceIndex\":{},\"spaceName\":\"{}\",\"level\":{},\"nextLevel\":{},\"label\":\"{}\",\"nextLabel\":\"{}\",\"buildCost\":{},\"rentAfter\":{},\"canBuild\":{}}}",
                    space.index,
                    escape_json(&space.name),
                    level,
                    next_level,
                    escape_json(building_label(level)),
                    escape_json(building_label(next_level)),
                    cost,
                    rent_after,
                    can_build
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn asset_actions_json(&self) -> String {
        if self.phase == Phase::TokenSelection || self.auction.is_some() {
            return String::new();
        }
        self.spaces
            .iter()
            .filter_map(|space| self.owners[space.index].map(|owner| (space, owner)))
            .filter(|(_, owner)| *owner == self.current_player_index || self.players[*owner].cash < 0)
            .map(|space| {
                let (space, player_index) = space;
                let can_mortgage = !self.mortgaged[space.index] && !self.has_buildings_in_group(space.index);
                let can_unmortgage = self.mortgaged[space.index]
                    && self.players[player_index].cash >= self.unmortgage_cost(space.index);
                let can_sell_building = self.buildings[space.index] > 0;
                format!(
                    "{{\"spaceIndex\":{},\"spaceName\":\"{}\",\"owner\":\"{}\",\"kind\":\"{}\",\"buildings\":{},\"mortgaged\":{},\"mortgageValue\":{},\"unmortgageCost\":{},\"sellValue\":{},\"canMortgage\":{},\"canUnmortgage\":{},\"canSellBuilding\":{}}}",
                    space.index,
                    escape_json(&space.name),
                    escape_json(&self.players[player_index].name),
                    escape_json(&space.kind),
                    self.buildings[space.index],
                    self.mortgaged[space.index],
                    self.mortgage_value(space.index),
                    self.unmortgage_cost(space.index),
                    space.build_cost.unwrap_or(0) / 2,
                    can_mortgage,
                    can_unmortgage,
                    can_sell_building
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn bank_proposals_json(&self) -> String {
        self.bank_proposals
            .iter()
            .map(|proposal| {
                let requester = &self.players[proposal.requester_index].name;
                let counterparty = &self.players[proposal.counterparty_index].name;
                let awaiting = &self.players[proposal.awaiting_player_index].name;
                format!(
                    "{{\"id\":{},\"kind\":\"{}\",\"requester\":\"{}\",\"counterparty\":\"{}\",\"awaitingPlayer\":\"{}\",\"cashFromRequester\":{},\"cashFromCounterparty\":{},\"spacesFromRequester\":[{}],\"spacesFromCounterparty\":[{}],\"summary\":\"{}\",\"note\":\"{}\"}}",
                    proposal.id,
                    escape_json(&proposal.kind),
                    escape_json(requester),
                    escape_json(counterparty),
                    escape_json(awaiting),
                    proposal.cash_from_requester,
                    proposal.cash_from_counterparty,
                    self.proposal_spaces_json(&proposal.spaces_from_requester),
                    self.proposal_spaces_json(&proposal.spaces_from_counterparty),
                    escape_json(&self.bank_proposal_summary(proposal)),
                    escape_json(&proposal.note)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn proposal_spaces_json(&self, indexes: &[usize]) -> String {
        indexes
            .iter()
            .filter_map(|index| self.spaces.get(*index))
            .map(|space| {
                format!(
                    "{{\"index\":{},\"name\":\"{}\"}}",
                    space.index,
                    escape_json(&space.name)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn drawn_card_json(&self) -> String {
        let Some(card) = &self.drawn_card else {
            return "null".to_string();
        };
        format!(
            "{{\"deck\":\"{}\",\"id\":\"{}\",\"title\":\"{}\",\"text\":\"{}\",\"icon\":\"{}\"}}",
            escape_json(&card.deck),
            escape_json(&card.id),
            escape_json(&card.title),
            escape_json(&card.text),
            escape_json(&card.icon)
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

    fn from_str(value: &str) -> Self {
        match value {
            "play" => Phase::Play,
            "auction" => Phase::Auction,
            _ => Phase::TokenSelection,
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
            controller: PlayerController::Human,
            cash: 15000,
            position: 0,
            token: None,
            jailed: false,
            jail_turns: 0,
            bankrupt: false,
        }
    }
}

impl PlayerController {
    fn as_str(self) -> &'static str {
        match self {
            PlayerController::Human => "human",
            PlayerController::Ai => "ai",
        }
    }

    fn from_str(value: &str) -> Self {
        if value == "ai" {
            PlayerController::Ai
        } else {
            PlayerController::Human
        }
    }
}

impl AiTurnAnswer {
    fn from_llm_answer(answer: &str) -> Option<Self> {
        let decision = AiDecision::from_llm_answer(answer)?;
        let reason = json_string_field(answer, "reason")
            .or_else(|| json_string_field(answer, "motivering"))
            .map(|reason| clean_ai_reason(&reason))
            .filter(|reason| !reason.is_empty())
            .unwrap_or_else(|| "Ingen tydlig motivering från modellen.".to_string());
        Some(Self { decision, reason })
    }
}

impl AiDecision {
    fn as_str(self) -> &'static str {
        match self {
            AiDecision::Buy => "buy",
            AiDecision::Decline => "decline",
            AiDecision::Roll => "roll",
            AiDecision::PayJail => "pay_jail",
            AiDecision::Liquidate => "liquidate",
            AiDecision::Bankrupt => "bankrupt",
            AiDecision::Build => "build",
            AiDecision::Trade => "trade",
            AiDecision::AcceptProposal => "accept_proposal",
            AiDecision::DeclineProposal => "decline_proposal",
            AiDecision::Wait => "wait",
        }
    }

    fn label_sv(self) -> &'static str {
        match self {
            AiDecision::Buy => "köp",
            AiDecision::Decline => "avstå",
            AiDecision::Roll => "tärningsslag",
            AiDecision::PayJail => "betala fängelse",
            AiDecision::Liquidate => "sälja/panta",
            AiDecision::Bankrupt => "konkurs",
            AiDecision::Build => "bygga",
            AiDecision::Trade => "handelsbud",
            AiDecision::AcceptProposal => "acceptera förslag",
            AiDecision::DeclineProposal => "avböja förslag",
            AiDecision::Wait => "vänta",
        }
    }

    fn from_action(value: &str) -> Option<Self> {
        match normalize_lookup(value).as_str() {
            "buy" | "kop" | "kopa" | "purchase" => Some(AiDecision::Buy),
            "decline" | "avsta" | "skip" | "auktion" => Some(AiDecision::Decline),
            "roll" | "sla" | "tarning" => Some(AiDecision::Roll),
            "payjail" | "payjailfine" | "betalafangelse" | "fangelse" => Some(AiDecision::PayJail),
            "liquidate" | "sell" | "mortgage" | "pant" | "salj" | "salja" => {
                Some(AiDecision::Liquidate)
            }
            "bankrupt" | "konkurs" => Some(AiDecision::Bankrupt),
            "build" | "bygg" | "bygga" | "house" | "hotel" | "hus" | "hotell" => {
                Some(AiDecision::Build)
            }
            "trade" | "byt" | "byta" | "byte" | "deal" | "bud" | "buda" => Some(AiDecision::Trade),
            "acceptproposal" | "accept" | "acceptera" | "godkann" | "godkanna" => {
                Some(AiDecision::AcceptProposal)
            }
            "declineproposal" | "reject" | "avbojj" | "avboj" | "neka" => {
                Some(AiDecision::DeclineProposal)
            }
            "wait" | "vanta" => Some(AiDecision::Wait),
            _ => None,
        }
    }

    fn from_llm_answer(answer: &str) -> Option<Self> {
        if let Some(action) = json_string_field(answer, "action") {
            if let Some(decision) = Self::from_action(&action) {
                return Some(decision);
            }
        }
        let normalized = normalize_lookup(answer);
        for (needle, decision) in [
            ("payjail", AiDecision::PayJail),
            ("betalafangelse", AiDecision::PayJail),
            ("liquidate", AiDecision::Liquidate),
            ("mortgage", AiDecision::Liquidate),
            ("bankrupt", AiDecision::Bankrupt),
            ("konkurs", AiDecision::Bankrupt),
            ("decline", AiDecision::Decline),
            ("avsta", AiDecision::Decline),
            ("buy", AiDecision::Buy),
            ("kop", AiDecision::Buy),
            ("acceptproposal", AiDecision::AcceptProposal),
            ("accept", AiDecision::AcceptProposal),
            ("acceptera", AiDecision::AcceptProposal),
            ("declineproposal", AiDecision::DeclineProposal),
            ("reject", AiDecision::DeclineProposal),
            ("avboj", AiDecision::DeclineProposal),
            ("build", AiDecision::Build),
            ("bygg", AiDecision::Build),
            ("trade", AiDecision::Trade),
            ("byt", AiDecision::Trade),
            ("roll", AiDecision::Roll),
            ("sla", AiDecision::Roll),
            ("wait", AiDecision::Wait),
            ("vanta", AiDecision::Wait),
        ] {
            if normalized.contains(needle) {
                return Some(decision);
            }
        }
        None
    }

    fn constrained_for_fallback(self, fallback: AiDecision) -> Self {
        if self == AiDecision::Wait {
            fallback
        } else {
            self
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

fn redirect(location: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    ).into_bytes()
}

fn html(status: u16, body: &str) -> Vec<u8> {
    asset(status, "text/html; charset=utf-8", body)
}

fn json(status: u16, body: &str) -> Vec<u8> {
    asset(status, "application/json; charset=utf-8", body)
}

fn asset(status: u16, content_type: &str, body: &str) -> Vec<u8> {
    binary_asset(status, content_type, body.as_bytes())
}

fn binary_asset(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        _ => "OK",
    };

    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store, no-cache, must-revalidate, max-age=0\r\nPragma: no-cache\r\nExpires: 0\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
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

fn optional_json_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_string_field(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = json.find(&pattern)? + pattern.len();
    let mut result = String::new();
    let mut escaped = false;
    for character in json[start..].chars() {
        if escaped {
            match character {
                'n' => result.push('\n'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                other => result.push(other),
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(result);
        } else {
            result.push(character);
        }
    }
    None
}

fn call_ollama_generate(
    url: &str,
    model: &str,
    prompt: &str,
    json_mode: bool,
    num_predict: u16,
) -> std::io::Result<String> {
    let (host, port, path) = parse_http_url(url).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bara http:// stöds för LLM",
        )
    })?;
    let format_field = if json_mode {
        ",\"format\":\"json\""
    } else {
        ""
    };
    let body = format!(
        "{{\"model\":\"{}\",\"prompt\":\"{}\",\"stream\":false{},\"options\":{{\"temperature\":0.25,\"num_predict\":{}}}}}",
        escape_json(model),
        escape_json(prompt),
        format_field,
        num_predict
    );
    let timeout = Duration::from_secs(llm_timeout_secs());
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "ogiltigt LLM-svar"))?;
    json_string_field(body, "response").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "LLM-svar saknar response")
    })
}

fn parse_http_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host_port, path) = match rest.split_once('/') {
        Some((host_port, path)) => (host_port, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().ok()?),
        None => (host_port.to_string(), 80),
    };
    Some((host, port, path))
}

fn snapshot_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn snapshot_unescape(value: &str) -> String {
    let mut result = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            match character {
                't' => result.push('\t'),
                'n' => result.push('\n'),
                '\\' => result.push('\\'),
                other => result.push(other),
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    result
}

fn snapshot_usize_list(values: &[usize]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_snapshot_usize_list(value: &str) -> Vec<usize> {
    value
        .split(',')
        .filter_map(|part| part.parse::<usize>().ok())
        .collect()
}

fn parse_bool(value: &str) -> bool {
    value == "true"
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

fn parse_game_cards(toml: &str) -> (Vec<GameCard>, Vec<GameCard>) {
    let mut chance = Vec::new();
    let mut community = Vec::new();
    let mut current_deck = String::new();
    let mut current = Vec::new();

    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[[chance]]" || trimmed == "[[community]]" {
            if !current.is_empty() {
                push_card_block(&mut chance, &mut community, &current_deck, &current);
                current.clear();
            }
            current_deck = trimmed
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .to_string();
        } else if !trimmed.is_empty() {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        push_card_block(&mut chance, &mut community, &current_deck, &current);
    }

    (chance, community)
}

fn push_card_block(
    chance: &mut Vec<GameCard>,
    community: &mut Vec<GameCard>,
    deck: &str,
    lines: &[String],
) {
    let card = parse_card_block(deck, lines);
    if deck == "chance" {
        chance.push(card);
    } else {
        community.push(card);
    }
}

fn parse_card_block(deck: &str, lines: &[String]) -> GameCard {
    let mut id = String::new();
    let mut title = String::new();
    let mut text = String::new();
    let mut icon = deck.to_string();
    let mut effect = String::new();
    let mut amount = None;
    let mut target = None;

    for line in lines {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "id" => id = trim_toml_quotes(value).to_string(),
            "title" => title = trim_toml_quotes(value).to_string(),
            "text" => text = trim_toml_quotes(value).to_string(),
            "icon" => icon = trim_toml_quotes(value).to_string(),
            "effect" => effect = trim_toml_quotes(value).to_string(),
            "amount" => amount = value.parse().ok(),
            "target" => target = value.parse().ok(),
            _ => {}
        }
    }

    GameCard {
        deck: deck.to_string(),
        id,
        title,
        text,
        icon,
        effect,
        amount,
        target,
    }
}

fn parse_space_block(lines: &[String]) -> BoardSpace {
    let mut index = 0;
    let mut kind = String::new();
    let mut name = String::new();
    let mut color = None;
    let mut price = None;
    let mut rent = None;
    let mut build_cost = None;
    let mut house_rents = Vec::new();
    let mut amount = None;
    let mut target = None;
    let mut card_title = None;
    let mut card_text = None;
    let mut card_icon = None;

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
            "build_cost" => build_cost = value.parse().ok(),
            "house_rents" => house_rents = parse_toml_i32_list(value),
            "amount" => amount = value.parse().ok(),
            "target" => target = value.parse().ok(),
            "card_title" => card_title = Some(trim_toml_quotes(value).to_string()),
            "card_text" => card_text = Some(trim_toml_quotes(value).to_string()),
            "card_icon" => card_icon = Some(trim_toml_quotes(value).to_string()),
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
        build_cost,
        house_rents,
        amount,
        target,
        card_title,
        card_text,
        card_icon,
    }
}

fn trim_toml_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn parse_toml_i32_list(value: &str) -> Vec<i32> {
    value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(|inner| {
            inner
                .split(',')
                .filter_map(|part| part.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default()
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
    if let Some(ai_turn_prompt) = toml_multiline_value(&toml, "turn_prompt") {
        settings.ai_turn_prompt = ai_turn_prompt.trim().to_string();
    }
    if let Some(ai_profiles_toml) = toml_literal_multiline_value(&toml, "profiles_toml") {
        settings.ai_profiles_toml = ai_profiles_toml.trim().to_string();
    }
    settings
}

fn load_bank_rules() -> BankRules {
    let toml =
        fs::read_to_string(BANK_RULES_PATH).unwrap_or_else(|_| DEFAULT_BANK_RULES_TOML.to_string());
    BankRules {
        emergency_loan_limit: toml_i32_value(&toml, "emergency_loan_limit").unwrap_or(1000),
        emergency_loan_required_requests: toml_i32_value(&toml, "emergency_loan_required_requests")
            .unwrap_or(2)
            .max(1) as usize,
        player_loan_limit: toml_i32_value(&toml, "player_loan_limit").unwrap_or(5000),
        donation_limit: toml_i32_value(&toml, "donation_limit").unwrap_or(10000),
        trade_cash_limit: toml_i32_value(&toml, "trade_cash_limit").unwrap_or(50000),
    }
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

fn toml_i32_value(toml: &str, key: &str) -> Option<i32> {
    let prefix = format!("{key} = ");
    toml.lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .and_then(|value| value.trim().parse::<i32>().ok())
}

fn toml_multiline_value(toml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"\"\"");
    let start = toml.find(&prefix)? + prefix.len();
    let rest = &toml[start..];
    let end = rest.find("\"\"\"")?;
    Some(rest[..end].to_string())
}

fn toml_literal_multiline_value(toml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = '''");
    let start = toml.find(&prefix)? + prefix.len();
    let rest = &toml[start..];
    let end = rest.find("'''")?;
    Some(rest[..end].to_string())
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_toml_multiline(value: &str) -> String {
    value.replace("\"\"\"", "\\\"\\\"\\\"")
}

fn escape_toml_literal_multiline(value: &str) -> String {
    value.replace("'''", "")
}

fn token_label(token: &str) -> &'static str {
    TOKEN_CHOICES
        .iter()
        .find(|(id, _)| *id == token)
        .map(|(_, label)| *label)
        .unwrap_or("pjäsen")
}

fn clean_player_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(24)
        .collect()
}

fn clean_chat_message(message: &str) -> String {
    message
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(280)
        .collect()
}

fn clean_ai_reason(message: &str) -> String {
    message
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(180)
        .collect()
}

fn parse_ai_names(names: &str) -> Vec<String> {
    names
        .split(',')
        .map(clean_player_name)
        .filter(|name| !name.is_empty())
        .collect()
}

fn ai_personality_for_player(player_name: &str, profiles_toml: &str) -> Option<String> {
    let target = normalize_lookup(player_name);
    for block in profile_toml_blocks(profiles_toml) {
        let Some(name) = toml_string_value(&block, "name") else {
            continue;
        };
        if normalize_lookup(&name) != target {
            continue;
        }
        if let Some(prompt) = toml_multiline_value(&block, "prompt")
            .or_else(|| toml_multiline_value(&block, "personality"))
            .or_else(|| toml_string_value(&block, "prompt"))
            .or_else(|| toml_string_value(&block, "personality"))
        {
            let prompt = prompt.trim().to_string();
            if !prompt.is_empty() {
                return Some(prompt);
            }
        }
    }
    None
}

fn profile_toml_blocks(toml: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[[profile]]" || trimmed == "[[ai_player]]" || trimmed == "[[ai_players]]" {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
        }
        if !trimmed.is_empty() {
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }
    blocks
}

fn default_ai_name(index: usize) -> String {
    ["Rut", "Bosse", "Sigrid", "Loke", "Eira"]
        .get(index)
        .copied()
        .unwrap_or("AI")
        .to_string()
}

fn normalize_lookup(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn extract_first_amount(message: &str) -> Option<i32> {
    let mut digits = String::new();
    let mut started = false;

    for character in message.chars() {
        if character.is_ascii_digit() {
            started = true;
            digits.push(character);
        } else if started && (character == ' ' || character == '.' || character == '_') {
            continue;
        } else if started {
            break;
        }
    }

    digits.parse::<i32>().ok()
}

fn looks_like_emergency_loan(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("nödlån")
        || lower.contains("nødlån")
        || lower.contains("nodlan")
        || lower.contains("nöd lån")
        || lower.contains("nödlana")
        || lower.contains("akut lån")
        || lower.contains("akutlån")
        || (lower.contains("lån")
            && (lower.contains("banken") || lower.contains("nöd") || lower.contains("akut")))
}

fn looks_like_exit_or_bankruptcy(message: &str) -> bool {
    let lower = message.to_lowercase();
    contains_any_word(message, &["konkurs"])
        || lower.contains("gå ur")
        || lower.contains("gar ur")
        || lower.contains("går ur")
        || lower.contains("lämna spelet")
        || lower.contains("lamna spelet")
        || lower.contains("hoppa av")
        || lower.contains("ger upp")
        || lower.contains("ge upp")
}

fn contains_any_word(message: &str, words: &[&str]) -> bool {
    message
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .any(|part| words.iter().any(|word| part == *word))
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "ingenting ännu".to_string()
    } else {
        items.join(", ")
    }
}

fn building_label(level: u8) -> &'static str {
    match level {
        0 => "ingen byggnad",
        1 => "1 hus",
        2 => "2 hus",
        3 => "3 hus",
        4 => "4 hus",
        _ => "hotell",
    }
}

fn color_group_label(color: &str) -> &'static str {
    match color {
        "brown" => "bruna",
        "light_blue" => "ljusblå",
        "pink" => "rosa",
        "orange" => "orange",
        "red" => "röda",
        "yellow" => "gula",
        "green" => "gröna",
        "blue" => "blå",
        _ => "färg",
    }
}

fn random_index(max: usize) -> usize {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    nanos % max
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn auction_seconds_left(auction: &AuctionState) -> u128 {
    let anchor = auction.started_at_ms.max(auction.last_bid_at_ms);
    let elapsed = now_millis().saturating_sub(anchor);
    AUCTION_MIN_MS.saturating_sub(elapsed).div_ceil(1000)
}

fn seed_rng(salt: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let seed = nanos ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    if seed == 0 {
        0xA076_1D64_78BD_642F
    } else {
        seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playable_game() -> GameState {
        let mut game = GameState::new();
        game.phase = Phase::Play;
        game.current_player_index = 0;
        game
    }

    #[test]
    fn builds_house_when_player_owns_complete_color_group() {
        let mut game = playable_game();
        game.owners[1] = Some(0);
        game.owners[3] = Some(0);
        let cash_before = game.players[0].cash;

        game.build_property(1, "");

        assert_eq!(game.buildings[1], 1);
        assert_eq!(game.players[0].cash, cash_before - 500);
        assert_eq!(game.rent_for_space(1), 100);
        assert!(game.bank_message.contains("bygger 1 hus"));
    }

    #[test]
    fn requires_even_building_inside_color_group() {
        let mut game = playable_game();
        for index in [6, 8, 9] {
            game.owners[index] = Some(0);
        }

        game.build_property(6, "");
        game.build_property(6, "");

        assert_eq!(game.buildings[6], 1);
        assert!(game.bank_message.contains("Bygg jämnt"));

        game.build_property(8, "");
        assert_eq!(game.buildings[8], 1);
    }

    #[test]
    fn rejects_action_from_wrong_mobile_player() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();

        game.roll_current_player("Noel");

        assert_eq!(game.players[0].position, 0);
        assert!(game.bank_message.contains("Majas tur"));
    }

    #[test]
    fn dice_rolls_use_two_independent_values() {
        let mut game = playable_game();
        game.rng_state = 0x1234_5678_9ABC_DEF0;
        let mut saw_non_double = false;

        for _ in 0..12 {
            game.dice = [game.roll_die(), game.roll_die()];
            if game.dice[0] != game.dice[1] {
                saw_non_double = true;
                break;
            }
        }

        assert!(saw_non_double, "dice should not always be doubles");
    }

    #[test]
    fn bank_chat_answers_with_player_context() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();

        let job = game
            .ask_bank("Maja", "Var står jag?")
            .expect("casual bank chat should run asynchronously");

        assert!(
            game.bank_chat
                .iter()
                .any(|message| message.speaker == "Maja")
        );
        assert!(
            game.bank_chat
                .iter()
                .any(|message| message.from_bank && message.text.contains("tänker"))
        );
        assert!(job.fallback_answer.contains("Gå"));
        assert!(game.bank_message.contains("Banken till Maja"));
    }

    #[test]
    fn emergency_loan_requires_repeated_request_even_out_of_turn() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();
        game.current_player_index = 1;
        let cash_before = game.players[0].cash;

        let first = game.ask_bank("Maja", "Jag behöver nödlån 1000 kr från banken");
        assert!(first.is_none());
        assert_eq!(game.players[0].cash, cash_before);
        assert!(game.bank_message.contains("tjata"));

        let second = game.ask_bank("Maja", "Nödlån 1000 kr från banken, snälla");
        assert!(second.is_none());
        assert_eq!(game.players[0].cash, cash_before + 1000);
        assert!(game.bank_message.contains("nödlån på 1000 kr"));
    }

    #[test]
    fn new_game_can_configure_ai_players() {
        let game = GameState::new_with_ai_players(4, 2, "Rut, Bosse");

        assert!(game.players[0].controller == PlayerController::Human);
        assert!(game.players[1].controller == PlayerController::Human);
        assert!(game.players[2].controller == PlayerController::Ai);
        assert!(game.players[3].controller == PlayerController::Ai);
        assert_eq!(game.players[2].name, "Rut");
        assert_eq!(game.players[3].name, "Bosse");
        assert!(game.bank_message.contains("2 AI-spelare"));
    }

    #[test]
    fn ai_profile_toml_matches_player_name() {
        let profile = ai_personality_for_player(
            "Bosse",
            r#"
[[profile]]
name = "Rut"
prompt = """
Försiktig.
"""

[[profile]]
name = "Bosse"
prompt = """
Offensiv och bytesglad.
"""
"#,
        )
        .unwrap();

        assert!(profile.contains("Offensiv"));
    }

    #[test]
    fn ai_decision_parses_llm_json_answer() {
        let decision =
            AiDecision::from_llm_answer(r#"{"action":"pay_jail","reason":"dyrt att vänta"}"#);

        assert!(decision == Some(AiDecision::PayJail));
    }

    #[test]
    fn ai_player_auto_selects_token_when_queued() {
        let mut game = GameState::new_with_ai_players(3, 1, "Rut");
        game.selection_order = vec![2, 0, 1];
        game.selection_cursor = 0;

        assert!(game.queue_ai_action_if_needed());
        game.run_ai_action();
        game.ai_action_pending = false;

        assert!(game.players[2].token.is_some());
        assert_eq!(game.selection_cursor, 1);
        assert!(game.players[2].controller == PlayerController::Ai);
    }

    #[test]
    fn ai_player_buys_pending_property_when_affordable() {
        let mut game = playable_game();
        game.players[0].name = "Rut".to_string();
        game.players[0].controller = PlayerController::Ai;
        game.pending_offer = Some(PendingOffer {
            player_index: 0,
            space_index: 1,
        });
        let cash_before = game.players[0].cash;
        let price = game.spaces[1].price.unwrap_or(0);

        game.run_ai_action();

        assert_eq!(game.owners[1], Some(0));
        assert_eq!(game.players[0].cash, cash_before - price);
    }

    #[test]
    fn stopped_game_blocks_ai_queue_and_player_actions() {
        let mut game = playable_game();
        game.players[0].controller = PlayerController::Ai;
        game.stop_game();

        assert!(!game.queue_ai_action_if_needed());
        if !game.reject_stopped_action() {
            game.roll_current_player("");
        }

        assert_eq!(game.players[0].position, 0);
        assert!(game.bank_message.contains("stoppat"));
    }

    #[test]
    fn ai_player_can_create_trade_proposal_for_color_group() {
        let mut game = playable_game();
        game.players[0].name = "Rut".to_string();
        game.players[1].name = "Bosse".to_string();
        game.players[0].controller = PlayerController::Ai;
        game.owners[1] = Some(0);
        game.owners[3] = Some(1);

        assert!(game.ai_propose_trade_once(0));

        assert_eq!(game.bank_proposals.len(), 1);
        assert_eq!(game.bank_proposals[0].requester_index, 0);
        assert_eq!(game.bank_proposals[0].counterparty_index, 1);
        assert_eq!(game.bank_proposals[0].spaces_from_counterparty, vec![3]);
    }

    #[test]
    fn bank_chat_returns_async_job_without_blocking_game_actions() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();

        let job = game
            .ask_bank("Maja", "Kan du prata lite med mig medan de andra spelar?")
            .expect("casual chat should be async");

        assert!(job.prompt.contains("Maja"));
        assert!(game.bank_message.contains("Spelet fortsätter"));
        assert!(game.phase == Phase::Play);
    }

    #[test]
    fn bank_creates_and_accepts_property_offer() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();
        game.owners[23] = Some(1);
        let maja_cash = game.players[0].cash;
        let noel_cash = game.players[1].cash;

        let job = game.ask_bank(
            "Maja",
            "Jag ser att Noel har Strandpromenaden. Lägg ett bud på 2200 kr.",
        );

        assert!(job.is_none());
        assert_eq!(game.bank_proposals.len(), 1);
        let proposal_id = game.bank_proposals[0].id;
        assert_eq!(game.bank_proposals[0].awaiting_player_index, 1);
        assert_eq!(game.bank_proposals[0].cash_from_requester, 2200);
        assert_eq!(game.bank_proposals[0].spaces_from_counterparty, vec![23]);

        game.accept_bank_proposal("Noel", proposal_id);

        assert_eq!(game.owners[23], Some(0));
        assert_eq!(game.players[0].cash, maja_cash - 2200);
        assert_eq!(game.players[1].cash, noel_cash + 2200);
        assert!(game.bank_proposals.is_empty());
    }

    #[test]
    fn real_bank_broker_actions_wait_for_player_turn() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();
        game.current_player_index = 1;
        game.owners[23] = Some(1);

        let job = game.ask_bank("Maja", "Lägg ett bud på Strandpromenaden");

        assert!(job.is_none());
        assert!(game.bank_proposals.is_empty());
        assert!(game.bank_message.contains("riktiga bud"));
    }

    #[test]
    fn street_name_with_kop_does_not_trigger_purchase_intent() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();

        let job = game
            .ask_bank("Maja", "Vad är hyran på Köpmangatan?")
            .expect("street lookup should remain casual async chat");

        assert!(game.bank_proposals.is_empty());
        assert!(job.fallback_answer.contains("hyra") || job.fallback_answer.contains("Gå"));
    }

    #[test]
    fn negative_player_cannot_roll_until_debt_is_resolved() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[0].cash = -200;

        game.roll_current_player("Maja");

        assert_eq!(game.players[0].position, 0);
        assert!(game.bank_message.contains("back"));
        assert!(game.bank_message.contains("nästa tärningsslag"));
    }

    #[test]
    fn negative_player_can_mortgage_assets_out_of_turn() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();
        game.current_player_index = 1;
        game.players[0].cash = -200;
        game.owners[1] = Some(0);

        game.mortgage_property(1, "Maja");

        assert!(game.mortgaged[1]);
        assert_eq!(game.players[0].cash, 100);
    }

    #[test]
    fn unresolved_debt_does_not_auto_bankrupt_player() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[0].cash = -20_000;

        game.resolve_negative_cash(0);

        assert!(!game.players[0].bankrupt);
        assert!(game.bank_message.contains("Begär konkurs"));
    }

    #[test]
    fn bank_bankruptcy_returns_assets_to_bank() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[0].cash = -500;
        game.owners[23] = Some(0);

        let job = game.ask_bank("Maja", "Jag går i konkurs");

        assert!(job.is_none());
        assert!(game.players[0].bankrupt);
        assert_eq!(game.owners[23], None);
        assert!(game.bank_message.contains("återgår till banken"));
    }

    #[test]
    fn game_over_json_summarizes_winner_when_one_player_remains() {
        let mut game = playable_game();
        game.players[0].name = "Anna".to_string();
        game.players[1].name = "Karin".to_string();
        game.players[0].cash = 12345;
        game.owners[1] = Some(0);
        game.owners[3] = Some(0);
        game.buildings[1] = 2;
        game.players[1].bankrupt = true;
        game.players[2].bankrupt = true;
        game.players[3].bankrupt = true;
        game.push_event("Karin gick i konkurs. Fastigheterna återgår till banken.".to_string());

        let json = game.game_over_json();

        assert!(json.contains("\"winner\":\"Anna\""));
        assert!(json.contains("\"propertyCount\":2"));
        assert!(json.contains("Karin gick i konkurs"));
        assert!(json.contains("\"name\":\"Karin\""));
    }

    #[test]
    fn voluntary_exit_can_give_assets_to_named_player() {
        let mut game = playable_game();
        game.players[0].name = "Maja".to_string();
        game.players[1].name = "Noel".to_string();
        game.players[0].cash = -500;
        game.owners[23] = Some(0);

        let job = game.ask_bank("Maja", "Jag går ur spelet och ger allt till Noel");

        assert!(job.is_none());
        assert!(game.players[0].bankrupt);
        assert_eq!(game.owners[23], Some(1));
        assert!(game.bank_message.contains("frivilligt utträde"));
    }

    #[test]
    fn mortgage_blocks_rent_and_unmortgage_costs_interest() {
        let mut game = playable_game();
        game.owners[1] = Some(0);
        let cash_before = game.players[0].cash;

        game.mortgage_property(1, "");

        assert!(game.mortgaged[1]);
        assert_eq!(game.players[0].cash, cash_before + 300);
        assert_eq!(game.rent_for_space(1), 0);

        game.unmortgage_property(1, "");
        assert!(!game.mortgaged[1]);
        assert_eq!(game.players[0].cash, cash_before + 300 - 330);
    }

    #[test]
    fn station_and_utility_rent_scale_by_owned_group() {
        let mut game = playable_game();
        game.owners[5] = Some(0);
        game.owners[15] = Some(0);
        game.owners[12] = Some(1);
        game.owners[27] = Some(1);
        game.dice = [3, 4];

        assert_eq!(game.rent_for_space(5), 500);
        assert_eq!(game.rent_for_space(12), 700);
    }

    #[test]
    fn auction_cannot_finish_before_countdown() {
        let mut game = playable_game();
        let now = now_millis();
        game.auction = Some(AuctionState {
            space_index: 1,
            seller_turn_index: 0,
            highest_bid: 100,
            highest_bidder: Some(1),
            started_at_ms: now,
            last_bid_at_ms: now,
        });
        game.phase = Phase::Auction;

        game.finish_auction();

        assert!(game.auction.is_some());
        assert!(game.bank_message.contains("sekunder till"));

        if let Some(auction) = &mut game.auction {
            auction.started_at_ms = now_millis().saturating_sub(AUCTION_MIN_MS + 1000);
            auction.last_bid_at_ms = auction.started_at_ms;
        }

        game.finish_auction();

        assert!(game.auction.is_none());
        assert_eq!(game.owners[1], Some(1));
    }

    #[test]
    fn paying_jail_fine_releases_player() {
        let mut game = playable_game();
        game.players[0].jailed = true;
        game.players[0].jail_turns = 1;
        let cash_before = game.players[0].cash;

        game.pay_jail_fine("");

        assert!(!game.players[0].jailed);
        assert_eq!(game.players[0].jail_turns, 0);
        assert_eq!(game.players[0].cash, cash_before - JAIL_FINE);
        assert_eq!(game.free_parking_pot, JAIL_FINE);
        assert!(game.bank_message.contains("Fri parkering-potten"));
    }

    #[test]
    fn free_parking_pot_pays_out_and_resets() {
        let mut game = playable_game();
        game.free_parking_pot = 1200;
        let cash_before = game.players[0].cash;

        let payout = game.collect_free_parking_pot(0);

        assert_eq!(payout, 1200);
        assert_eq!(game.players[0].cash, cash_before + 1200);
        assert_eq!(game.free_parking_pot, 0);
    }

    #[test]
    fn snapshot_roundtrip_preserves_demo_state() {
        let game = GameState::demo();
        let snapshot = game.to_snapshot();
        let loaded = GameState::from_snapshot(&snapshot).expect("snapshot should load");

        assert_eq!(loaded.players[0].name, "Maja");
        assert_eq!(loaded.players[1].jailed, true);
        assert_eq!(loaded.owners[1], Some(0));
        assert_eq!(loaded.buildings[21], 2);
        assert_eq!(loaded.mortgaged[5], true);
        assert_eq!(loaded.free_parking_pot, 1300);
        assert!(loaded.bank_message.contains("Demo-läge"));
    }
}
