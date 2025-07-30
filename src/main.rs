use actix_web::cookie::{time::Duration, Cookie};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use askama::Template;
use clap::Parser;
use rusqlite::{params, Connection};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{server, Channel, ChannelId, CryptoVec};
use serde::Deserialize;
use serde::Serialize;
use serde_yaml;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    http_port: u16,
    ssh_port: u16,
    chat_name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_port: 8080,
            ssh_port: 2222,
            chat_name: "NoJS Chat".to_string(),
        }
    }
}

#[derive(Parser)]
#[command(name = "nojs-chat", about = "Minimal chat server over HTTP and SSH")]
struct Args {
    /// HTTP port
    #[arg(short = 'p', long = "port")]
    http_port: Option<u16>,

    /// SSH port
    #[arg(short = 's', long = "ssh")]
    ssh_port: Option<u16>,

    /// Chat name
    #[arg(short = 'n', long = "name")]
    chat_name: Option<String>,

    /// Path to config file
    #[arg(short = 'c', long = "config", default_value = "config.yml")]
    config: String,
}

struct AppState {
    conn: Mutex<Connection>,
    config: Config,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate<'a> {
    chat_name: &'a str,
}

#[derive(Template)]
#[template(path = "register.html")]
struct RegisterTemplate<'a> {
    chat_name: &'a str,
}

struct ChatMessage {
    username: String,
    content: String,
}

#[derive(Clone)]
struct SshServer {
    data: web::Data<AppState>,
    clients: Arc<AsyncMutex<HashMap<usize, (String, ChannelId, russh::server::Handle)>>>,
    id: usize,
    username: Option<String>,
}

impl SshServer {
    async fn broadcast(&self, msg: &str) {
        let mut clients = self.clients.lock().await;
        let data = CryptoVec::from(format!("\r\n{}\r\n> ", msg));
        for (_, (_, channel, handle)) in clients.iter_mut() {
            let _ = handle.data(*channel, data.clone()).await;
        }
    }
}

impl server::Server for SshServer {
    type Handler = Self;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        let mut new = self.clone();
        new.id = self.id + 1;
        self.id += 1;
        new
    }

    fn handle_session_error(&mut self, _error: <Self::Handler as server::Handler>::Error) {
        eprintln!("Session error: {:?}", _error);
    }
}

impl server::Handler for SshServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let conn = self.data.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id FROM users WHERE username=?1 AND password=?2")
            .unwrap();
        let ok: Option<i64> = stmt
            .query_row(params![user, password], |row| row.get(0))
            .ok();
        if ok.is_some() {
            self.username = Some(user.to_string());
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        {
            let mut clients = self.clients.lock().await;
            clients.insert(
                self.id,
                (
                    self.username.clone().unwrap_or_default(),
                    channel.id(),
                    session.handle(),
                ),
            );
        }

        // Simple TUI welcome screen
        session.data(channel.id(), CryptoVec::from("\x1b[2J\x1b[H"))?;
        if let Some(name) = &self.username {
            let welcome = format!("Welcome, {}! Type /help for commands.\r\n", name);
            session.data(channel.id(), CryptoVec::from(welcome))?;
        }

        // Send chat history
        if let Some(name) = &self.username {
            let history = {
                let conn = self.data.conn.lock().unwrap();
                let mut stmt = conn
                    .prepare(
                        "SELECT users.username, messages.content FROM messages JOIN users ON users.id = messages.user_id ORDER BY messages.ts DESC LIMIT 20",
                    )
                    .unwrap();
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .unwrap();
                let mut vec = Vec::new();
                for r in rows {
                    vec.push(r.unwrap());
                }
                vec
            };

            for (u, c) in history {
                let data = CryptoVec::from(format!("{}: {}\r\n", u, c));
                session.data(channel.id(), data)?;
            }

            let join_msg = format!("* {} joined", name);
            self.broadcast(&join_msg).await;
            session.data(channel.id(), CryptoVec::from("> "))?;
        }
        Ok(true)
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if data == [3] {
            return Err(russh::Error::Disconnect);
        }
        let msg = String::from_utf8_lossy(data).trim().to_string();
        if msg.is_empty() {
            return Ok(());
        }
        if msg == "/help" {
            let help = "Commands:\n/help - this help\n/quit - exit chat\n";
            let clients = self.clients.lock().await;
            if let Some((_, channel, handle)) = clients.get(&self.id) {
                let _ = handle.data(*channel, CryptoVec::from(help)).await;
                let _ = handle.data(*channel, CryptoVec::from("> ")).await;
            }
            return Ok(());
        }
        if msg == "/quit" {
            return Err(russh::Error::Disconnect);
        }
        if let Some(name) = &self.username {
            {
                let conn = self.data.conn.lock().unwrap();
                let mut stmt = conn
                    .prepare("SELECT id FROM users WHERE username=?1")
                    .unwrap();
                if let Ok(uid) = stmt.query_row(params![name], |row| row.get::<_, i64>(0)) {
                    let _ = conn.execute(
                        "INSERT INTO messages (user_id, content) VALUES (?1, ?2)",
                        params![uid, msg.clone()],
                    );
                }
            }
            let full = format!("{}: {}", name, msg);
            self.broadcast(&full).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.close(channel)?;
        {
            let mut clients = self.clients.lock().await;
            clients.remove(&self.id);
        }
        if let Some(name) = &self.username {
            let leave = format!("* {} left", name);
            self.broadcast(&leave).await;
        }
        Ok(())
    }
}

#[derive(Template)]
#[template(path = "chat.html")]
struct ChatTemplate<'a> {
    chat_name: &'a str,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct MessageForm {
    content: String,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mut config: Config = std::fs::read_to_string(&args.config)
        .ok()
        .and_then(|c| serde_yaml::from_str(&c).ok())
        .unwrap_or_default();

    if let Some(p) = args.http_port {
        config.http_port = p;
    }
    if let Some(p) = args.ssh_port {
        config.ssh_port = p;
    }
    if let Some(n) = args.chat_name {
        config.chat_name = n;
    }

    println!(
        "Starting {} on http port {} and ssh port {}",
        config.chat_name, config.http_port, config.ssh_port
    );

    let conn = Connection::open("chat.db").expect("open db");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, username TEXT UNIQUE, password TEXT)",
        [],
    ).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, user_id INTEGER, content TEXT, ts DATETIME DEFAULT CURRENT_TIMESTAMP)",
        [],
    ).unwrap();

    let data = web::Data::new(AppState {
        conn: Mutex::new(conn),
        config: config.clone(),
    });

    // Start SSH server in background
    let ssh_data = data.clone();
    let ssh_port = config.ssh_port;
    tokio::spawn(async move {
        let server_conf = russh::server::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![russh::keys::PrivateKey::random(
                &mut rand_core::OsRng,
                russh::keys::Algorithm::Ed25519,
            )
            .unwrap()],
            ..Default::default()
        };
        let config_arc = Arc::new(server_conf);
        let mut server = SshServer {
            data: ssh_data,
            clients: Arc::new(AsyncMutex::new(HashMap::new())),
            id: 0,
            username: None,
        };
        let _ = server
            .run_on_address(config_arc, ("0.0.0.0", ssh_port))
            .await;
    });

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .service(web::resource("/").route(web::get().to(index)))
            .service(web::resource("/login").route(web::post().to(login)))
            .service(
                web::resource("/register")
                    .route(web::get().to(register_page))
                    .route(web::post().to(register)),
            )
            .service(web::resource("/chat").route(web::get().to(chat_page)))
            .service(web::resource("/message").route(web::post().to(post_message)))
            .service(web::resource("/logout").route(web::get().to(logout)))
    })
    .bind(("0.0.0.0", config.http_port))?
    .run()
    .await
}

fn get_user_id(req: &HttpRequest) -> Option<i64> {
    req.cookie("user_id").and_then(|c| c.value().parse().ok())
}

async fn index(req: HttpRequest, data: web::Data<AppState>) -> impl Responder {
    if get_user_id(&req).is_some() {
        HttpResponse::Found()
            .append_header(("Location", "/chat"))
            .finish()
    } else {
        HttpResponse::Ok().content_type("text/html").body(
            LoginTemplate {
                chat_name: &data.config.chat_name,
            }
            .render()
            .unwrap(),
        )
    }
}

async fn register_page(data: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().content_type("text/html").body(
        RegisterTemplate {
            chat_name: &data.config.chat_name,
        }
        .render()
        .unwrap(),
    )
}

async fn register(form: web::Form<LoginForm>, data: web::Data<AppState>) -> impl Responder {
    let conn = data.conn.lock().unwrap();
    let _ = conn.execute(
        "INSERT INTO users (username, password) VALUES (?1, ?2)",
        params![form.username, form.password],
    );
    HttpResponse::Found()
        .append_header(("Location", "/"))
        .finish()
}

async fn login(form: web::Form<LoginForm>, data: web::Data<AppState>) -> impl Responder {
    let conn = data.conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id FROM users WHERE username=?1 AND password=?2")
        .unwrap();
    let user_id: Option<i64> = stmt
        .query_row(params![form.username, form.password], |row| row.get(0))
        .ok();
    if let Some(id) = user_id {
        let mut resp = HttpResponse::Found()
            .append_header(("Location", "/chat"))
            .finish();
        let cookie = Cookie::build("user_id", id.to_string()).path("/").finish();
        resp.add_cookie(&cookie).unwrap();
        resp
    } else {
        HttpResponse::Found()
            .append_header(("Location", "/"))
            .finish()
    }
}

async fn chat_page(req: HttpRequest, data: web::Data<AppState>) -> impl Responder {
    if get_user_id(&req).is_some() {
        let conn = data.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT users.username, messages.content FROM messages JOIN users ON users.id = messages.user_id ORDER BY messages.ts DESC LIMIT 20",
        ).unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(ChatMessage {
                    username: row.get(0)?,
                    content: row.get(1)?,
                })
            })
            .unwrap();
        let mut messages = Vec::new();
        for r in rows {
            messages.push(r.unwrap());
        }
        HttpResponse::Ok().content_type("text/html").body(
            ChatTemplate {
                chat_name: &data.config.chat_name,
                messages,
            }
            .render()
            .unwrap(),
        )
    } else {
        HttpResponse::Found()
            .append_header(("Location", "/"))
            .finish()
    }
}

async fn post_message(
    req: HttpRequest,
    form: web::Form<MessageForm>,
    data: web::Data<AppState>,
) -> impl Responder {
    if let Some(user_id) = get_user_id(&req) {
        let conn = data.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO messages (user_id, content) VALUES (?1, ?2)",
            params![user_id, form.content],
        );
    }
    HttpResponse::Found()
        .append_header(("Location", "/chat"))
        .finish()
}

async fn logout() -> impl Responder {
    let mut resp = HttpResponse::Found()
        .append_header(("Location", "/"))
        .finish();
    let cookie = Cookie::build("user_id", "")
        .path("/")
        .max_age(Duration::seconds(0))
        .finish();
    resp.add_cookie(&cookie).unwrap();
    resp
}
