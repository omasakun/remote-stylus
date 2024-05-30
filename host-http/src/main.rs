use std::net::IpAddr;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use warp::{
  filters::ws::{Message, WebSocket, Ws},
  http::header::HeaderValue,
  path::Tail,
  reply::Response,
  Filter, Rejection, Reply,
};

#[derive(Embed)]
#[folder = "web/"]
struct Asset;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
  #[arg(long, default_value = "127.0.0.1")]
  host: String,

  #[arg(long, default_value_t = 8080)]
  port: u16,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  let index_html = warp::path::end().and_then(|| async { serve_asset("index.html") });
  let assets = warp::path::tail().and_then(|path: Tail| async move { serve_asset(path.as_str()) });

  let websocket = warp::path("ws")
    .and(warp::ws())
    .map(|ws: Ws| ws.on_upgrade(handle_websocket));

  let routes = index_html.or(assets).or(websocket);
  warp::serve(routes)
    .run((args.host.parse::<IpAddr>().unwrap(), args.port))
    .await;
}

async fn handle_websocket(ws: WebSocket) {
  let (mut tx, mut rx) = ws.split();

  tokio::spawn(async move {
    while let Some(result) = rx.next().await {
      match result {
        Ok(msg) => {
          if let Ok(text) = msg.to_str() {
            println!("Received: {}", text);
            tx.send(Message::text(text)).await.unwrap();
          }
        }
        Err(e) => {
          eprintln!("WebSocket error: {}", e);
          break;
        }
      }
    }
  });
}
fn serve_asset(path: &str) -> Result<impl Reply, Rejection> {
  let asset = Asset::get(path).ok_or_else(warp::reject::not_found)?;
  let mime = mime_guess::from_path(path).first_or_octet_stream();

  let mut res = Response::new(asset.data.into());
  res.headers_mut().insert(
    "content-type",
    HeaderValue::from_str(mime.as_ref()).unwrap(),
  );
  Ok(res)
}
