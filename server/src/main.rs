use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

const TV_HTML: &str = include_str!("../../web/tv/index.html");
const MOBILE_HTML: &str = include_str!("../../web/mobile/index.html");
const ADMIN_HTML: &str = include_str!("../../web/admin/index.html");
const STYLES_CSS: &str = include_str!("../../web/shared/styles.css");
const APP_JS: &str = include_str!("../../web/shared/app.js");

fn main() -> std::io::Result<()> {
    let bind_addr = env::var("EUTHERPAL_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let listener = TcpListener::bind(&bind_addr)?;

    println!("EutherPal dev server listening on http://{bind_addr}");
    println!("TV:     http://{bind_addr}/tv");
    println!("Mobile: http://{bind_addr}/mobile");
    println!("Admin:  http://{bind_addr}/admin");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| {
                    if let Err(error) = handle_connection(stream) {
                        eprintln!("request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    let response = match path {
        "/" => redirect("/tv"),
        "/health" => json(200, r#"{"status":"ok","service":"eutherpal","ai":"mock"}"#),
        "/api/game/mock" => json(200, mock_game_json()),
        "/tv" | "/tv/" => html(200, TV_HTML),
        "/mobile" | "/mobile/" => html(200, MOBILE_HTML),
        "/admin" | "/admin/" => html(200, ADMIN_HTML),
        "/assets/styles.css" => asset(200, "text/css; charset=utf-8", STYLES_CSS),
        "/assets/app.js" => asset(200, "application/javascript; charset=utf-8", APP_JS),
        _ => html(404, "<h1>404</h1><p>Sidan finns inte.</p>"),
    };

    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
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

fn mock_game_json() -> &'static str {
    r#"{
  "roomCode": "PAL-001",
  "phase": "testläge",
  "currentPlayer": "Anna",
  "bankMessage": "Välkommen till EutherPal. Anna börjar. Banken är i mockläge tills LLM-tunneln är inkopplad.",
  "dice": [3, 4],
  "players": [
    {"name": "Anna", "cash": 15000, "position": 7, "token": "bil"},
    {"name": "Bo", "cash": 15000, "position": 0, "token": "hatt"},
    {"name": "Cleo", "cash": 15000, "position": 18, "token": "skepp"}
  ],
  "spaces": [
    "Gå", "Södra Vägen", "Allmänning", "Norra Gränd", "Inkomstskatt",
    "Central", "Björkallén", "Chans", "Kvarnbacken", "Hamngatan",
    "Fängelse", "Skolgatan", "Elverket", "Torggatan", "Parkvägen",
    "Västra stationen", "Strandgatan", "Allmänning", "Kyrkbacken", "Slottsgatan",
    "Fri parkering", "Solvägen", "Chans", "Månstigen", "Stjärnallén",
    "Norra station", "Regnbågen", "Vindverket", "Bergsgatan", "Kungsgatan",
    "Gå i fängelse", "Apoteket", "Euthergatan", "Allmänning", "Teknikparken",
    "Östra station", "Chans", "Serverhall", "Lyxskatt", "Apothic Avenue"
  ]
}"#
}
