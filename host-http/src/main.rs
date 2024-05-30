use std::{net::IpAddr, process::Stdio};

use clap::Parser;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::{
  io::AsyncReadExt,
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
  res
    .headers_mut()
    .insert("content-type", HeaderValue::from_str(mime.as_ref()).unwrap());
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

      // TODO: use desktop duplication api to capture screen and pass it to ffmpeg

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
        let mut slicer = slicer::Slicer::new();
        let mut buf = vec![0; 1024];

        loop {
          let n = video.read(&mut buf).await.unwrap();
          if n == 0 {
            break;
          }
          let chunks = slicer.append(&buf[..n]);
          for chunk in chunks {
            let _ = tx_video.send(chunk).await;
          }
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

// Split video data into 'initialization segment' and 'media segments', and send them separately
mod slicer {
  // https://qiita.com/tomoyukilabs/items/57ba8a982ab372611669
  // https://qiita.com/ryiwamoto/items/0ff451da6ab76b4f4064
  // https://www.matroska.org/files/matroska_file_format_alexander_noe.pdf
  // https://inza.blog/2014/04/30/ebml-extensible-binary-meta-language/
  // https://www.matroska.org/technical/basics.html

  const TAG_EBML: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];
  const TAG_SEGMENT: [u8; 4] = [0x18, 0x53, 0x80, 0x67];
  const TAG_CLUSTER: [u8; 4] = [0x1f, 0x43, 0xb6, 0x75];
  const TAG_VOID: [u8; 1] = [0xec];

  enum State {
    Header,
    Data,
  }

  pub struct Slicer {
    state: State,
    buffer: Vec<u8>,
  }
  impl Slicer {
    pub fn new() -> Self {
      Self {
        state: State::Header,
        buffer: Vec::new(),
      }
    }
    pub fn append(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
      let buf = &mut self.buffer;
      buf.extend_from_slice(data);

      match self.state {
        State::Header => {
          let mut offset = 0;

          // find the EBML element
          if !buf[offset..].starts_with(&TAG_EBML) {
            assert!(buf.len() < offset + TAG_EBML.len(), "EBML not found");
            return vec![];
          }
          offset += TAG_EBML.len();
          offset += read_varint(buf, &mut offset);

          // find the Segment element
          if !buf[offset..].starts_with(&TAG_SEGMENT) {
            assert!(buf.len() < offset + TAG_SEGMENT.len(), "Segment not found");
            return vec![];
          }
          offset += TAG_SEGMENT.len();
          assert!(read_varint(buf, &mut offset) == 0xff_ffff_ffff_ffff); // unknown size

          // find the Cluster element
          loop {
            if buf.len() < offset + 1 {
              return vec![];
            }
            if buf[offset..].starts_with(&TAG_CLUSTER) {
              break;
            }
            if buf[offset..].starts_with(&TAG_VOID) {
              // println!("Void");
              offset += TAG_VOID.len();
              offset += read_varint(buf, &mut offset);
            } else {
              // println!("Unknown {:x?}", &buf[offset..offset + 4]);
              offset += 4;
              offset += read_varint(buf, &mut offset);
            }
          }

          self.state = State::Data;
          // println!("Header found");
          let data = buf.drain(..offset).collect();
          vec![data]
        }
        State::Data => {
          let mut offset = 0;

          // find the Cluster element
          if !buf[offset..].starts_with(&TAG_CLUSTER) {
            assert!(buf.len() < offset + TAG_CLUSTER.len(), "Cluster not found");
            return vec![];
          }
          offset += TAG_CLUSTER.len();
          offset += read_varint(buf, &mut offset);

          if buf.len() < offset {
            return vec![];
          }

          let data: Vec<u8> = buf.drain(..offset).collect();
          // println!("Cluster found: {}", data.len());
          vec![data]
        }
      }
    }
  }

  // unsigned integer with variable length
  fn read_varint(data: &[u8], offset: &mut usize) -> usize {
    let len = data[*offset].leading_zeros() as usize + 1;
    let mut value = 0;
    for i in 0..len {
      value = (value << 8) | data[*offset + i] as u64;
      if i == 0 {
        value -= 1 << (8 - len);
      }
    }
    // println!("Varint: {:?} -> {:x}", &data[*offset..*offset + len], value);
    *offset += len;
    value as usize
  }
}
