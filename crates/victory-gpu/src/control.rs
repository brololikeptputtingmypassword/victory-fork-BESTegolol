//! Debugging aid, only in builds with diagnostics: `VICTORY_CONTROL=<folder>`
//! lets a tool drive and inspect the title while it runs. Lines appended to
//! `<folder>/in.txt` are commands; what they answer is appended to
//! `<folder>/out.txt`, each answer ended by a line `# <command number>`.
//!
//! - `pad [a=1] [lt=0.5] [rx=-1] ... [ms=500]`: what the pad holds until the
//!   next `pad` (or for that long); `pad` alone lets go.
//! - `tap <buttons, with +> [ms]`: pressed on top of that.
//! - `capture [frames]`: the next frames are captured (`crate::capture`).
//! - `shot`: the picture of a frame that follows is saved.
//! - `where`: the frame and the time.
//! - `set <switch> <0|1>`, `unset <switch>`: a switch read by [`switch`].
//! - `peek <address> <bytes>`, `mem <address> <bytes> <file>`: title memory.
//! - `slot <index>...`: what is known of slots of the view heap.
//! - `audit [lines]`: which textures picked by index show other memory than
//!   the title's is now.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::descriptor::View;
use crate::object::This;

/// The folder, if driven.
pub fn folder() -> Option<&'static PathBuf> {
    static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOLDER.get_or_init(|| std::env::var_os("VICTORY_CONTROL").map(PathBuf::from)).as_ref()
}

/// Starts the thread that reads commands, once, if driven.
pub fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    let Some(folder) = folder() else { return };
    STARTED.get_or_init(|| {
        let _ = std::fs::create_dir_all(folder);
        let (from, to) = (folder.join("in.txt"), folder.join("out.txt"));
        let _ = std::fs::write(&from, "");
        let _ = std::fs::write(&to, "");
        let _ = std::thread::Builder::new().name("victory-control".into()).spawn(move || {
            let (mut at, mut rest, mut count) = (0u64, String::new(), 0u64);
            loop {
                std::thread::sleep(Duration::from_millis(30));
                let Ok(mut file) = std::fs::File::open(&from) else { continue };
                if file.metadata().is_ok_and(|m| m.len() <= at) || file.seek(SeekFrom::Start(at)).is_err() {
                    continue;
                }
                let mut read = Vec::new();
                if file.read_to_end(&mut read).is_err() {
                    continue;
                }
                at += read.len() as u64;
                rest.push_str(&String::from_utf8_lossy(&read));
                while let Some(end) = rest.find('\n') {
                    let line: String = rest.drain(..=end).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    count += 1;
                    let answer = std::panic::catch_unwind(|| command(line)).unwrap_or_else(|_| "panicked".into());
                    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&to) {
                        let _ = write!(file, "{}\n# {count}\n", answer.trim_end());
                    }
                }
            }
        });
        tracing::info!(folder = %folder.display(), "driven by commands");
    });
}

/// What a command holds down: the pad's buttons by name, its axes
/// (triggers, left stick, right stick).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pad {
    pub buttons: Vec<String>,
    /// lt, rt, lx, ly, rx, ry
    pub axes: [f32; 6],
}

const AXES: [&str; 6] = ["lt", "rt", "lx", "ly", "rx", "ry"];

/// What is held, until when.
static PADS: Mutex<Vec<(Pad, Option<Instant>)>> = Mutex::new(Vec::new());

/// What commands hold down now.
pub fn pad() -> Option<Pad> {
    let mut pads = PADS.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    pads.retain(|(_, until)| until.is_none_or(|until| now < until));
    let mut all = Pad::default();
    for (pad, _) in pads.iter() {
        all.buttons.extend(pad.buttons.iter().cloned());
        for (sum, axis) in all.axes.iter_mut().zip(pad.axes) {
            if axis != 0.0 {
                *sum = axis;
            }
        }
    }
    (!pads.is_empty()).then_some(all)
}

/// `a=1 lt=0.5 ms=300` as what to hold and for how long.
fn parse_pad(words: &[&str]) -> Result<(Pad, Option<u64>), String> {
    let (mut pad, mut ms) = (Pad::default(), None);
    for word in words {
        let (name, value) = word.split_once('=').unwrap_or((word, "1"));
        let name = name.to_ascii_lowercase();
        let number: f32 = value.parse().map_err(|_| format!("not a number: {word}"))?;
        if name == "ms" {
            ms = Some(number as u64);
        } else if let Some(i) = AXES.iter().position(|a| *a == name) {
            pad.axes[i] = number.clamp(-1.0, 1.0);
        } else if number != 0.0 {
            pad.buttons.push(name);
        }
    }
    Ok((pad, ms))
}

static SWITCHES: RwLock<Option<HashMap<String, bool>>> = RwLock::new(None);

/// A switch of the running title: what a command set, or `default`.
pub(crate) fn switch(name: &str, default: bool) -> bool {
    let set = SWITCHES.read().unwrap_or_else(|e| e.into_inner());
    set.as_ref().and_then(|set| set.get(name).copied()).unwrap_or(default)
}

fn number(text: &str) -> Result<u64, String> {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    }
    .map_err(|_| format!("not a number: {text}"))
}

fn read_title(address: u64, bytes: u64) -> Vec<u8> {
    let mut out = vec![0u8; bytes.min(256 << 20) as usize];
    // Read through the kernel: pages not there stay zero.
    crate::host::copy_mapped(out.as_mut_ptr(), address as *const u8, out.len(), false);
    out
}

fn command(line: &str) -> String {
    run(line).unwrap_or_else(|why| format!("error: {why}"))
}

fn run(line: &str) -> Result<String, String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let missing = || "words missing".to_string();
    match words[0] {
        "pad" => {
            let (pad, ms) = parse_pad(&words[1..])?;
            let mut pads = PADS.lock().unwrap_or_else(|e| e.into_inner());
            pads.retain(|(_, until)| until.is_some());
            if pad != Pad::default() {
                pads.push((pad, ms.map(|ms| Instant::now() + Duration::from_millis(ms))));
            }
            Ok("ok".into())
        }
        "tap" => {
            let names = words.get(1).ok_or_else(missing)?;
            let ms = words.get(2).map_or(Ok(150), |w| number(w))?;
            let names: Vec<&str> = names.split('+').collect();
            let (pad, _) = parse_pad(&names)?;
            PADS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((pad, Some(Instant::now() + Duration::from_millis(ms))));
            Ok("ok".into())
        }
        "capture" => {
            let frames = words.get(1).map_or(Ok(1), |w| number(w))?;
            let first = crate::queue::presented() + 4;
            crate::capture::next(first, frames);
            Ok(format!("frames {first}+{frames}"))
        }
        "shot" => {
            let frame = crate::queue::presented() + 2;
            crate::capture::SHOT.store(frame, Relaxed);
            Ok(format!("frame-{frame}.png"))
        }
        "where" => Ok(format!("frame {} at {} ms", crate::queue::presented(), crate::host::now_ms())),
        "set" => {
            let name = words.get(1).ok_or_else(missing)?;
            let on = words.get(2).is_none_or(|v| *v != "0");
            let mut set = SWITCHES.write().unwrap_or_else(|e| e.into_inner());
            set.get_or_insert_default().insert(name.to_string(), on);
            tracing::info!(name, on, frame = crate::queue::presented(), "switch set by a command");
            Ok("ok".into())
        }
        "unset" => {
            let name = words.get(1).ok_or_else(missing)?;
            if let Some(set) = SWITCHES.write().unwrap_or_else(|e| e.into_inner()).as_mut() {
                set.remove(*name);
            }
            Ok("ok".into())
        }
        "peek" => {
            let address = number(words.get(1).ok_or_else(missing)?)?;
            let bytes = number(words.get(2).ok_or_else(missing)?)?.min(1 << 20);
            let mut text = String::new();
            for byte in read_title(address, bytes) {
                let _ = write!(text, "{byte:02x}");
            }
            Ok(text)
        }
        "mem" => {
            let address = number(words.get(1).ok_or_else(missing)?)?;
            let bytes = number(words.get(2).ok_or_else(missing)?)?;
            let file = words.get(3).ok_or_else(missing)?;
            std::fs::write(file, read_title(address, bytes)).map_err(|e| e.to_string())?;
            Ok("ok".into())
        }
        "slot" => {
            let mut text = String::new();
            for word in &words[1..] {
                let index = number(word)? as u32;
                let _ = writeln!(text, "{}", slot(index));
            }
            Ok(text)
        }
        "audit" => Ok(audit(words.get(1).map_or(Ok(40), |w| number(w))? as usize)),
        other => Err(format!("no command {other}")),
    }
}

/// What the last upload of a texture picked by index left.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Upload {
    pub(crate) at: u64,
    pub(crate) len: u64,
    pub(crate) print: u64,
    pub(crate) copy: u64,
    pub(crate) frame: u64,
    /// With all of its memory there.
    pub(crate) whole: bool,
    /// Left to the draws that read it through a table.
    pub(crate) tabled: bool,
}

static UPLOADS: Mutex<Option<HashMap<u32, Upload>>> = Mutex::new(None);
/// The title's view heap and ours of it, of the last pass over the textures.
static HEAPS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

pub(crate) fn heaps(heap: &crate::descriptor::Heap, host: &crate::descriptor::host::HostHeap) {
    HEAPS[0].store(std::ptr::from_ref(heap) as u64, Relaxed);
    HEAPS[1].store(std::ptr::from_ref(host) as u64, Relaxed);
}

fn with_heaps<T>(with: impl FnOnce(&crate::descriptor::Heap, &crate::descriptor::host::HostHeap) -> T) -> Option<T> {
    let (heap, host) = (HEAPS[0].load(Relaxed), HEAPS[1].load(Relaxed));
    if heap == 0 || host == 0 {
        return None;
    }
    // SAFETY: the title makes one view heap and keeps it (debugging aid).
    Some(unsafe { with(&*(heap as *const crate::descriptor::Heap), &*(host as *const crate::descriptor::host::HostHeap)) })
}

/// Slot `index` was uploaded from title memory `[at, at + len)`.
pub(crate) fn uploaded(index: u32, upload: Upload) {
    let mut uploads = UPLOADS.lock().unwrap_or_else(|e| e.into_inner());
    uploads.get_or_insert_default().insert(index, upload);
}

/// The texture object a slot's view shows.
fn object(view: View) -> Option<This> {
    match crate::descriptor::host::placed_texture(&view).unwrap_or(view) {
        View::Shader(resource, _) => Some(resource as This),
        _ => None,
    }
}

fn slot(index: u32) -> String {
    let upload = UPLOADS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|u| u.get(&index).copied());
    let mut text = format!("slot {index}: ");
    let _ = with_heaps(|heap, host| {
        let view = heap.handle(index).and_then(crate::descriptor::view);
        let _ = write!(text, "{view:?}; queue {}", host.texture_slot_state(index));
        if let Some(View::Placed(false, resource, desc)) = &view {
            let address = u64::from(desc[10]) | u64::from(desc[11]) << 32;
            let len = crate::resource::placed_view_bytes(resource, desc).unwrap_or(0);
            let mapped = crate::texture::mapped_bytes(address as *const u8, len as usize);
            let _ = write!(text, "; memory {address:#x}+{len:#x}, mapped {mapped:#x}");
        }
        if let Some(object) = view.and_then(object) {
            let copy = crate::resource::copy_serial(object);
            let staged = crate::texture::is_staged(object);
            let _ = write!(text, "; object {object:?} copy {copy} staged {staged}");
        }
    });
    match upload {
        Some(u) => {
            let same = crate::texture::fingerprint(u.at, u.len) == u.print;
            let _ = write!(text, "; uploaded {u:x?} at frame {}, memory {}", u.frame, if same { "as then" } else { "CHANGED" });
        }
        None => text.push_str("; never uploaded by the pass"),
    }
    text
}

/// Every upload noted against the title's memory now.
fn audit(lines: usize) -> String {
    let uploads: Vec<(u32, Upload)> = {
        let uploads = UPLOADS.lock().unwrap_or_else(|e| e.into_inner());
        uploads.iter().flatten().map(|(&i, &u)| (i, u)).collect()
    };
    // [whole, partly, left to tables] x [as uploaded, changed, copy made anew]
    let mut counts = [[0u64; 3]; 3];
    let mut gone = 0u64;
    // Out of the queue with no host copy: these read as nothing.
    let mut dead = [0u64; 2];
    let mut changed: Vec<(u64, String)> = Vec::new();
    let now = crate::frame::current();
    let began = Instant::now();
    let total = uploads.len();
    for (index, u) in uploads {
        let state = with_heaps(|heap, host| {
            let view = heap.handle(index).and_then(crate::descriptor::view)?;
            Some((object(view).map_or(0, crate::resource::copy_serial), host.texture_slot_state(index)))
        });
        let Some(Some((copy, queue))) = state else {
            gone += 1;
            continue;
        };
        let kind = if u.tabled { 2 } else { usize::from(!u.whole) };
        let same = crate::texture::fingerprint(u.at, u.len) == u.print;
        if copy == 0 && queue == "none" {
            dead[usize::from(same)] += 1;
        }
        let what = match (same, copy == u.copy) {
            (false, _) => 1,
            (true, false) => 2,
            (true, true) => 0,
        };
        counts[kind][what] += 1;
        if what != 0 {
            let ago = now.saturating_sub(u.frame);
            changed.push((
                ago,
                format!(
                    "slot {index} {} {}: {:#x}+{:#x} uploaded {ago} frames ago, queue {queue}",
                    ["whole", "partly", "tabled"][kind],
                    ["", "CHANGED", "COPY ANEW"][what],
                    u.at,
                    u.len,
                ),
            ));
        }
    }
    changed.sort_by_key(|c| std::cmp::Reverse(c.0));
    let mut text = format!(
        "frame {now}: {total} uploads, {gone} slots without a view; [as uploaded, changed, copy made anew]: \
         whole {:?}, partly {:?}, left to tables {:?}; out of the queue without a copy [other memory, as uploaded] {dead:?}; {} ms\n",
        counts[0],
        counts[1],
        counts[2],
        began.elapsed().as_millis()
    );
    for (_, line) in changed.iter().take(lines) {
        let _ = writeln!(text, "{line}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_words() {
        let (pad, ms) = parse_pad(&["a", "lt=0.5", "ry=-2", "b=0", "ms=40"]).unwrap();
        assert_eq!(pad.buttons, ["a"]);
        assert_eq!(pad.axes, [0.5, 0.0, 0.0, 0.0, 0.0, -1.0]);
        assert_eq!(ms, Some(40));
        assert!(parse_pad(&["lt=x"]).is_err());
    }
}
