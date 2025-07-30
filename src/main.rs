use actix_web::cookie::{time::Duration, Cookie};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use askama::Template;
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::sync::Mutex;

struct AppState {
    conn: Mutex<Connection>,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate;

#[derive(Template)]
#[template(path = "register.html")]
struct RegisterTemplate;

struct ChatMessage {
    username: String,
    content: String,
}

#[derive(Template)]
#[template(path = "chat.html")]
struct ChatTemplate {
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
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

fn get_user_id(req: &HttpRequest) -> Option<i64> {
    req.cookie("user_id").and_then(|c| c.value().parse().ok())
}

async fn index(req: HttpRequest) -> impl Responder {
    if get_user_id(&req).is_some() {
        HttpResponse::Found()
            .append_header(("Location", "/chat"))
            .finish()
    } else {
        HttpResponse::Ok()
            .content_type("text/html")
            .body(LoginTemplate.render().unwrap())
    }
}

async fn register_page() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html")
        .body(RegisterTemplate.render().unwrap())
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
        HttpResponse::Ok()
            .content_type("text/html")
            .body(ChatTemplate { messages }.render().unwrap())
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
