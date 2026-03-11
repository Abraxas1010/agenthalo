use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::thread;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidecarEvent {
    pub channel: String,
    pub seq: u64,
    pub timestamp: u64,
    pub puf_digest: [u8; 32],
    pub payload: serde_json::Value,
}

pub fn run_sidecar_listener(path: &Path, tx: Sender<SidecarEvent>) -> std::io::Result<()> {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path)?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = handle_stream(stream, &tx);
        }
    });
    Ok(())
}

fn handle_stream(stream: UnixStream, tx: &Sender<SidecarEvent>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        if let Ok(event) = serde_json::from_str::<SidecarEvent>(line.trim()) {
            let _ = tx.send(event);
        }
    }
    Ok(())
}

pub fn send_event(stream: &mut UnixStream, event: &SidecarEvent) -> std::io::Result<()> {
    let payload = serde_json::to_vec(event).map_err(|e| std::io::Error::other(e.to_string()))?;
    stream.write_all(&payload)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
