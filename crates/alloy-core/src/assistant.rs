//! Alloy's Claude assistant, the parts that live in Alloy.
//!
//! AlloyMC's server holds the Anthropic key, the system prompt and the tool definitions, and
//! meters what each person uses. Alloy sends it the conversation, reads the streamed reply
//! back into whole content blocks, and runs the tools Claude asks for on this computer, so
//! nothing about the player's instances has to leave it except what a tool's answer says.
//!
//! This module has the relay client and the stream assembler, plus the file access the
//! file tools are allowed: text files in an instance's config, log and crash folders, and
//! writes only to configs.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::alloymc::{api_url, AlloyMc};
use crate::error::{Error, Result};
use crate::instance::Instance;
use crate::Launcher;

/// Whether this person may ask now, and how much of their allowance is used, from
/// `GET /v1/assistant`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub signed_in: bool,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
    /// "email" or "minecraft" when that's what's missing.
    #[serde(default)]
    pub needs: Option<String>,
    #[serde(default)]
    pub day_used: u32,
    #[serde(default)]
    pub month_used: u32,
    #[serde(default)]
    pub day_resets: Option<String>,
    #[serde(default)]
    pub unlimited: bool,
}

/// A piece of the reply as it streams in, for the chat to show while Claude writes.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Delta {
    /// More text in the text block at `index`.
    Text { index: usize, text: String },
    /// Claude started asking for a tool.
    Tool { index: usize, id: String, name: String },
}

/// One whole reply: Anthropic content blocks, ready to go back into the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub content: Vec<Value>,
    pub stop_reason: Option<String>,
}

impl AlloyMc {
    /// The assistant's status for the signed-in person.
    pub async fn assistant_status(&self) -> Result<Status> {
        let Some(token) = self.token() else {
            return Ok(Status { reason: Some("Sign in to AlloyMC to use the assistant.".into()), ..Default::default() });
        };
        let res = self.http().get(format!("{}/v1/assistant", api_url())).bearer_auth(token).send().await?;
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(Status { reason: Some("Sign in to AlloyMC again to use the assistant.".into()), ..Default::default() });
        }
        let mut st: Status = crate::alloymc::read(res).await?;
        st.signed_in = true;
        Ok(st)
    }

    /// Sends the conversation and streams Claude's reply. `mode` is "fast" or "deep";
    /// `messages` are Anthropic messages. `on_delta` sees the reply as it's written.
    pub async fn assistant_send(&self, mode: &str, messages: &Value, mut on_delta: impl FnMut(Delta)) -> Result<Reply> {
        let token = self.token().ok_or_else(|| Error::other("Sign in to AlloyMC to use the assistant."))?;
        let mut res = self
            .http()
            .post(format!("{}/v1/assistant/messages", api_url()))
            .bearer_auth(token)
            .json(&json!({ "mode": mode, "messages": messages }))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(crate::alloymc::api_error(res).await);
        }
        let mut asm = Assembler::default();
        let mut sse = Sse::default();
        while let Some(chunk) = res.chunk().await? {
            for ev in sse.push(&chunk) {
                asm.event(&ev, &mut on_delta)?;
            }
        }
        for ev in sse.finish() {
            asm.event(&ev, &mut on_delta)?;
        }
        asm.finish()
    }
}

/// Splits a server-sent event stream into the `data` of each event.
#[derive(Default)]
struct Sse {
    buf: String,
    pending: Vec<u8>,
}

impl Sse {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        // Chunks can end inside a multi-byte character; keep the tail for the next one.
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let text: Vec<u8> = self.pending.drain(..valid).collect();
        self.buf.push_str(&String::from_utf8_lossy(&text).replace("\r\n", "\n"));
        let mut out = vec![];
        while let Some(end) = self.buf.find("\n\n") {
            let block: String = self.buf.drain(..end + 2).collect();
            if let Some(d) = data_of(&block) {
                out.push(d);
            }
        }
        out
    }

    fn finish(&mut self) -> Vec<String> {
        let rest = std::mem::take(&mut self.buf);
        data_of(&rest).into_iter().collect()
    }
}

fn data_of(block: &str) -> Option<String> {
    let lines: Vec<&str> =
        block.lines().filter_map(|l| l.strip_prefix("data:")).map(|l| l.strip_prefix(' ').unwrap_or(l)).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Builds whole content blocks from Anthropic's stream events.
#[derive(Default)]
struct Assembler {
    blocks: Vec<Value>,
    /// Tool input JSON as it arrives, per block.
    partial: Vec<String>,
    stop: Option<String>,
    stopped: bool,
}

impl Assembler {
    fn event(&mut self, data: &str, on_delta: &mut impl FnMut(Delta)) -> Result<()> {
        let Ok(ev) = serde_json::from_str::<Value>(data) else { return Ok(()) };
        match ev["type"].as_str().unwrap_or_default() {
            "content_block_start" => {
                let i = ev["index"].as_u64().unwrap_or(self.blocks.len() as u64) as usize;
                let block = ev["content_block"].clone();
                while self.blocks.len() <= i {
                    self.blocks.push(Value::Null);
                    self.partial.push(String::new());
                }
                if block["type"] == "tool_use" {
                    on_delta(Delta::Tool {
                        index: i,
                        id: block["id"].as_str().unwrap_or_default().into(),
                        name: block["name"].as_str().unwrap_or_default().into(),
                    });
                } else if let Some(t) = block["text"].as_str().filter(|t| !t.is_empty()) {
                    on_delta(Delta::Text { index: i, text: t.into() });
                }
                self.blocks[i] = block;
            }
            "content_block_delta" => {
                let i = ev["index"].as_u64().unwrap_or_default() as usize;
                let Some(block) = self.blocks.get_mut(i) else { return Ok(()) };
                let d = &ev["delta"];
                match d["type"].as_str().unwrap_or_default() {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or_default();
                        let joined = format!("{}{t}", block["text"].as_str().unwrap_or_default());
                        block["text"] = joined.into();
                        on_delta(Delta::Text { index: i, text: t.into() });
                    }
                    "input_json_delta" => self.partial[i].push_str(d["partial_json"].as_str().unwrap_or_default()),
                    "thinking_delta" => {
                        let joined = format!("{}{}", block["thinking"].as_str().unwrap_or_default(), d["thinking"].as_str().unwrap_or_default());
                        block["thinking"] = joined.into();
                    }
                    "signature_delta" => block["signature"] = d["signature"].clone(),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let i = ev["index"].as_u64().unwrap_or_default() as usize;
                if let Some(block) = self.blocks.get_mut(i) {
                    if block["type"] == "tool_use" {
                        let raw = self.partial[i].trim();
                        block["input"] = if raw.is_empty() { json!({}) } else { serde_json::from_str(raw).unwrap_or_else(|_| json!({})) };
                    }
                }
            }
            "message_delta" => {
                if let Some(s) = ev["delta"]["stop_reason"].as_str() {
                    self.stop = Some(s.into());
                }
            }
            "message_stop" => self.stopped = true,
            "error" => {
                let kind = ev["error"]["type"].as_str().unwrap_or_default();
                return Err(Error::other(match kind {
                    "overloaded_error" => "Claude is very busy right now. Try again in a minute.".to_string(),
                    _ => format!("The assistant stopped: {}", ev["error"]["message"].as_str().unwrap_or("unknown error")),
                }));
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Reply> {
        if !self.stopped && self.stop.is_none() {
            return Err(Error::other("The connection to the assistant dropped. Try again."));
        }
        // Empty text blocks aren't accepted back; text the model never finished is fine.
        let content =
            self.blocks.into_iter().filter(|b| !b.is_null() && !(b["type"] == "text" && b["text"].as_str().unwrap_or_default().is_empty())).collect();
        Ok(Reply { content, stop_reason: self.stop })
    }
}

// ---------- files ----------

/// Folders (and files) inside an instance the assistant may read.
const READABLE: &[&str] = &["config", "defaultconfigs", "kubejs", "logs", "crash-reports", "options.txt"];
/// What it may write: configs only.
const WRITABLE: &[&str] = &["config", "defaultconfigs", "kubejs", "options.txt"];
/// Text files only, and only this much of one.
const READ_LIMIT: usize = 40_000;
const WRITE_LIMIT: usize = 512 * 1024;
const TEXT_EXT: &[&str] = &[
    "txt", "log", "json", "json5", "jsonc", "toml", "cfg", "conf", "properties", "yml", "yaml", "ini", "js", "ts", "zs", "snbt", "nbt.txt",
    "mcmeta", "csv", "md", "xml", "lang",
];

/// A path the assistant gave, checked: relative, no `..`, inside one of `roots`.
fn checked(inst_dir: &Path, rel: &str, roots: &[&str]) -> Result<PathBuf> {
    let rel = rel.trim().trim_start_matches("./").replace('\\', "/");
    let p = Path::new(&rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(Error::other(format!("\"{rel}\" isn't a path inside the instance.")));
    }
    let first = rel.split('/').next().unwrap_or_default();
    if !roots.contains(&first) {
        return Err(Error::other(format!("Only these are allowed: {}.", roots.join(", "))));
    }
    Ok(inst_dir.join(p))
}

fn is_text_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TEXT_EXT.iter().any(|e| lower.ends_with(&format!(".{e}"))) || !lower.contains('.')
}

/// Reads a text file in an instance, or lists a folder when `rel` ends in `/` or is one.
pub async fn read_file(launcher: &Launcher, inst: &Instance, rel: &str) -> Result<String> {
    let dir = inst.dir(launcher);
    let path = checked(&dir, rel.trim_end_matches('/'), READABLE)?;
    let meta = tokio::fs::metadata(&path).await.map_err(|_| Error::other(format!("{rel} doesn't exist.")))?;
    if meta.is_dir() {
        return list_dir(&dir, &path).await;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if name.ends_with(".gz") {
        return Err(Error::other("That's a compressed log. latest.log and crash reports can be read."));
    }
    let bytes = tokio::fs::read(&path).await?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(Error::other("That isn't a text file."));
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(cut(&text, READ_LIMIT))
}

async fn list_dir(inst_dir: &Path, path: &Path) -> Result<String> {
    let mut out = vec![];
    let mut rd = tokio::fs::read_dir(path).await?;
    while let Some(e) = rd.next_entry().await? {
        let Ok(m) = e.metadata().await else { continue };
        let rel = e.path().strip_prefix(inst_dir).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        out.push(if m.is_dir() { format!("{rel}/") } else { format!("{rel} ({} bytes)", m.len()) });
    }
    out.sort();
    let n = out.len();
    out.truncate(400);
    if n > 400 {
        out.push(format!("... and {} more", n - 400));
    }
    Ok(if out.is_empty() { "(empty folder)".into() } else { out.join("\n") })
}

/// The file's text now, for showing a change before it's made. `None` when it's new.
pub async fn current_text(launcher: &Launcher, inst: &Instance, rel: &str) -> Result<Option<String>> {
    let path = checked(&inst.dir(launcher), rel, WRITABLE)?;
    match tokio::fs::read(&path).await {
        Ok(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Writes a config file in an instance.
pub async fn write_file(launcher: &Launcher, inst: &Instance, rel: &str, content: &str) -> Result<()> {
    let path = checked(&inst.dir(launcher), rel, WRITABLE)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if !is_text_name(name) {
        return Err(Error::other("Only text config files can be written."));
    }
    if content.len() > WRITE_LIMIT {
        return Err(Error::other("That's too big for a config file."));
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::util::write_atomic(&path, content.as_bytes()).await
}

/// The end of `logs/latest.log`, optionally only lines containing `contains` (any case).
pub async fn read_log(launcher: &Launcher, inst: &Instance, lines: usize, contains: Option<&str>) -> Result<String> {
    let path = inst.dir(launcher).join("logs").join("latest.log");
    let bytes = tokio::fs::read(&path).await.map_err(|_| Error::other("This instance has no log yet. It's written when the game runs."))?;
    let text = String::from_utf8_lossy(&bytes);
    let needle = contains.map(|c| c.to_lowercase()).filter(|c| !c.is_empty());
    let picked: Vec<&str> = text.lines().filter(|l| needle.as_ref().is_none_or(|n| l.to_lowercase().contains(n))).collect();
    let n = lines.clamp(10, 400);
    let start = picked.len().saturating_sub(n);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("({start} earlier lines left out)\n"));
    }
    for l in &picked[start..] {
        // Some mods log enormous lines; keep each readable.
        out.push_str(&cut(l, 600));
        out.push('\n');
    }
    if picked.is_empty() {
        out = match needle {
            Some(n) => format!("No lines contain \"{n}\"."),
            None => "The log is empty.".into(),
        };
    }
    Ok(cut(&out, READ_LIMIT))
}

fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n... (cut at {max} characters of {})", &s[..end], s.len())
}

// ---------- conversations ----------

/// A saved conversation: the messages exactly as sent, and what the list shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub instance_id: Option<String>,
    pub updated: i64,
    #[serde(default)]
    pub messages: Vec<Value>,
    /// What each tool call did, in words, by tool use id.
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    pub title: String,
    pub instance_id: Option<String>,
    pub updated: i64,
}

fn chats_dir(launcher: &Launcher) -> PathBuf {
    launcher.dirs.root().join("assistant")
}

fn chat_path(launcher: &Launcher, id: &str) -> Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(Error::other("bad conversation id"));
    }
    Ok(chats_dir(launcher).join(format!("{id}.json")))
}

/// Saved conversations, newest first.
pub async fn chats(launcher: &Launcher) -> Vec<ChatSummary> {
    let mut out = vec![];
    let Ok(mut rd) = tokio::fs::read_dir(chats_dir(launcher)).await else { return out };
    while let Ok(Some(e)) = rd.next_entry().await {
        let Ok(b) = tokio::fs::read(e.path()).await else { continue };
        if let Ok(c) = serde_json::from_slice::<Chat>(&b) {
            out.push(ChatSummary { id: c.id, title: c.title, instance_id: c.instance_id, updated: c.updated });
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.updated));
    out
}

pub async fn chat(launcher: &Launcher, id: &str) -> Result<Chat> {
    let b = tokio::fs::read(chat_path(launcher, id)?).await.map_err(|_| Error::other("That conversation is gone."))?;
    Ok(serde_json::from_slice(&b)?)
}

pub async fn save_chat(launcher: &Launcher, c: &Chat) -> Result<()> {
    let path = chat_path(launcher, &c.id)?;
    tokio::fs::create_dir_all(chats_dir(launcher)).await?;
    crate::util::write_atomic(&path, &serde_json::to_vec(c)?).await
}

pub async fn delete_chat(launcher: &Launcher, id: &str) -> Result<()> {
    let _ = tokio::fs::remove_file(chat_path(launcher, id)?).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: ping\ndata: {\"type\":\"ping\"}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me look at \"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"the crash – ok.\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_crash\",\"input\":{}}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"instance_\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"id\\\": \\\"abc\\\"}\"}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"list_instances\",\"input\":{}}}\n\n\
event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":40}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    fn run(chunks: &[&[u8]]) -> (Result<Reply>, Vec<Delta>) {
        let mut deltas = vec![];
        let mut asm = Assembler::default();
        let mut sse = Sse::default();
        let mut on = |d: Delta| deltas.push(d);
        for c in chunks {
            for ev in sse.push(c) {
                if let Err(e) = asm.event(&ev, &mut on) {
                    return (Err(e), deltas);
                }
            }
        }
        for ev in sse.finish() {
            asm.event(&ev, &mut on).unwrap();
        }
        (asm.finish(), deltas)
    }

    #[test]
    fn assembles_text_and_tools() {
        let (reply, deltas) = run(&[STREAM.as_bytes()]);
        let reply = reply.unwrap();
        assert_eq!(reply.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(reply.content.len(), 3);
        assert_eq!(reply.content[0]["text"], "Let me look at the crash – ok.");
        assert_eq!(reply.content[1]["input"], json!({"instance_id": "abc"}));
        assert_eq!(reply.content[2]["input"], json!({}));
        assert_eq!(deltas[0], Delta::Text { index: 0, text: "Let me look at ".into() });
        assert!(deltas.contains(&Delta::Tool { index: 1, id: "toolu_1".into(), name: "read_crash".into() }));
    }

    #[test]
    fn chunks_split_anywhere() {
        // Every possible split, including inside the multi-byte dash and inside "\n\n".
        let bytes = STREAM.replace('\n', "\r\n").into_bytes();
        for at in (0..bytes.len()).step_by(7) {
            let (reply, _) = run(&[&bytes[..at], &bytes[at..]]);
            assert_eq!(reply.unwrap().content[0]["text"], "Let me look at the crash – ok.", "split at {at}");
        }
    }

    #[test]
    fn errors_and_dropped_streams() {
        let err = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let (r, _) = run(&[err.as_bytes()]);
        assert!(r.unwrap_err().to_string().contains("busy"));
        let cut_off = &STREAM[..STREAM.find("event: message_delta").unwrap()];
        let (r, _) = run(&[cut_off.as_bytes()]);
        assert!(r.unwrap_err().to_string().contains("dropped"));
    }

    #[test]
    fn paths_stay_inside() {
        let d = Path::new("/i");
        assert!(checked(d, "config/sodium-options.json", READABLE).is_ok());
        assert!(checked(d, "./options.txt", WRITABLE).is_ok());
        for bad in ["../secrets.json", "/etc/passwd", "config/../../x", "mods/a.jar", "", "C:\\x"] {
            assert!(checked(d, bad, WRITABLE).is_err(), "{bad}");
        }
        assert!(checked(d, "logs/latest.log", READABLE).is_ok());
        assert!(checked(d, "logs/latest.log", WRITABLE).is_err());
    }

    /// Through real HTTP: a stand-in for AlloyMC that streams the reply in pieces.
    #[tokio::test]
    async fn streams_over_http() {
        use crate::auth::SecretStore;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut req = vec![0u8; 65536];
            let n = sock.read(&mut req).await.unwrap();
            let head = String::from_utf8_lossy(&req[..n]).to_string();
            sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n").await.unwrap();
            for piece in STREAM.as_bytes().chunks(97) {
                sock.write_all(piece).await.unwrap();
                sock.flush().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            head
        });
        std::env::set_var("ALLOY_MC_API", format!("http://{addr}"));
        let tmp = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(crate::paths::Dirs::new(tmp.path())).await.unwrap();
        let store = std::sync::Arc::new(crate::auth::FileSecretStore::new(tmp.path().join("s.json")));
        store.set("alloymc:token", "tok123").unwrap();
        let mc = AlloyMc::new(launcher, store);
        let mut texts = String::new();
        let reply = mc
            .assistant_send("fast", &json!([{"role": "user", "content": "hi"}]), |d| {
                if let Delta::Text { text, .. } = d {
                    texts.push_str(&text)
                }
            })
            .await
            .unwrap();
        std::env::remove_var("ALLOY_MC_API");
        let head = server.await.unwrap();
        assert!(head.starts_with("POST /v1/assistant/messages"));
        assert!(head.to_lowercase().contains("authorization: bearer tok123"));
        assert!(head.contains(r#""mode":"fast""#));
        assert_eq!(texts, "Let me look at the crash – ok.");
        assert_eq!(reply.content[1]["name"], "read_crash");
    }

    #[tokio::test]
    async fn files_and_logs() {
        let tmp = tempfile::tempdir().unwrap();
        let launcher = Launcher::open(crate::paths::Dirs::new(tmp.path())).await.unwrap();
        let inst = crate::instance::create(
            &launcher,
            crate::instance::NewInstance {
                name: "Test".into(),
                game_version: "1.21.1".into(),
                loader: crate::meta::loaders::Loader::Vanilla,
                loader_version: None,
                icon: None,
                group: None,
                fabric_api: None,
            },
        )
        .await
        .unwrap();
        let dir = inst.dir(&launcher);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        let log: String = (0..100).map(|i| format!("[{i}] {}\n", if i % 10 == 0 { "ERROR boom" } else { "fine" })).collect();
        std::fs::write(dir.join("logs/latest.log"), log).unwrap();
        let out = read_log(&launcher, &inst, 20, Some("error")).await.unwrap();
        assert_eq!(out.lines().count(), 10);
        let out = read_log(&launcher, &inst, 10, None).await.unwrap();
        assert!(out.starts_with("(90 earlier"));

        write_file(&launcher, &inst, "config/a/b.toml", "x = 1").await.unwrap();
        assert_eq!(read_file(&launcher, &inst, "config/a/b.toml").await.unwrap(), "x = 1");
        assert!(read_file(&launcher, &inst, "config/").await.unwrap().contains("config/a/"));
        assert!(write_file(&launcher, &inst, "config/x.jar", "no").await.is_err());
        assert_eq!(current_text(&launcher, &inst, "config/new.json").await.unwrap(), None);
    }
}
