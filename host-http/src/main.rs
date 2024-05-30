use std::{net::IpAddr, process::Stdio};

use clap::Parser;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::{
  io::{AsyncReadExt, BufReader},
  process::Command,
  select,
  sync::{mpsc, watch},
};
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
  host: IpAddr,

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

  println!("Listening on http://{}:{}", args.host, args.port);
  warp::serve(routes).run((args.host, args.port)).await;
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
  Hello,
  Key { key: String },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum HostMessage {
  Video {
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
  },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
  Initial,
  Greeted,
  Closed,
}

async fn handle_websocket(ws: WebSocket) {
  let (mut tx, mut rx) = ws.split();
  let (tx_stage, rx_stage) = watch::channel(Stage::Initial);

  // handle incoming messages
  tokio::spawn({
    let rx_stage = rx_stage.clone();
    async move {
      while let Some(msg) = rx.next().await {
        let Ok(msg) = msg else {
          eprintln!("Failed to receive message");
          break;
        };
        let Ok(msg) = rmp_serde::from_slice::<ClientMessage>(msg.as_bytes()) else {
          eprintln!("Failed to decode message");
          break;
        };

        // initial message must be greeting
        if *rx_stage.borrow() == Stage::Initial {
          if let ClientMessage::Hello = msg {
            let _ = tx_stage.send(Stage::Greeted);
            continue;
          } else {
            eprintln!("Unexpected message");
            break;
          }
        }

        // handle subsequent messages
        match msg {
          ClientMessage::Hello => {
            panic!("Unexpected message");
          }
          ClientMessage::Key { key } => {
            println!("Received key: {}", key);
            // TODO
          }
        }
      }
      let _ = tx_stage.send(Stage::Closed);
    }
  });

  // send video data
  tokio::spawn({
    let mut rx_stage = rx_stage.clone();
    async move {
      if wait_for_greeting(&mut rx_stage).await.is_err() {
        return;
      }
      let (tx_video, mut rx_video) = mpsc::channel(1);

      // https://www.webmproject.org/docs/encoder-parameters/
      let ffmpeg = Command::new("ffmpeg")
        .args([
          // "-filter_complex",
          // "ddagrab=0,hwdownload,format=bgra",
          //
          "-f",
          "gdigrab",
          "-framerate",
          "30",
          "-video_size",
          "1920x1080",
          "-i",
          "desktop",
          //
          "-c:v",
          "libvpx",
          "-deadline",
          "realtime",
          "-pix_fmt",
          "yuv420p",
          "-f",
          "webm",
          "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();

      let Ok(mut ffmpeg) = ffmpeg else {
        eprintln!("Failed to start ffmpeg");
        return;
      };

      let mut video = ffmpeg.stdout.take().unwrap();

      tokio::spawn(async move {
        let mut buf = vec![0; 1024 * 1024];
        loop {
          let n = video.read(&mut buf).await.unwrap();
          if n == 0 {
            break;
          }
          let _ = tx_video.send(buf[..n].to_vec()).await;
        }
      });

      loop {
        select! {
          data = rx_video.recv() => {
            let Some(data) = data else {
              break;
            };
            let msg = HostMessage::Video { data };
            let _ = send_message(&mut tx, msg).await;
          }
          _ = rx_stage.changed() => {
            if *rx_stage.borrow_and_update() == Stage::Closed {
              break;
            }
          }
        }
      }

      let _ = ffmpeg.kill().await;
    }
  });
}

async fn wait_for_greeting(rx: &mut watch::Receiver<Stage>) -> Result<(), ()> {
  loop {
    match *rx.borrow_and_update() {
      Stage::Greeted => return Ok(()),
      Stage::Closed => return Err(()),
      _ => {}
    }
    rx.changed().await.unwrap();
  }
}

async fn send_message(tx: &mut SplitSink<WebSocket, Message>, msg: HostMessage) {
  let msg = rmp_serde::encode::to_vec_named(&msg).unwrap();
  let msg = Message::binary(msg);
  if let Err(e) = tx.send(msg).await {
    eprintln!("Failed to send message: {}", e);
  }
}
