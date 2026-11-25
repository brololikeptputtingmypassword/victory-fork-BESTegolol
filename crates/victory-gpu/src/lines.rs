//! Which title memory shaders really write (`VICTORY_READ_LEARN=1`).
//!
//! A submission that reads title memory on the CPU (the title's buffer
//! copies, fills of constants) waits for the flights whose shaders may
//! write it (`crate::host::late`). What they may write is what they
//! declare, and a title that binds its whole pool of constants as one
//! unordered buffer (GTA VI: 182 MB) declares all of it: each submission
//! then waited for the flight before it, to read constants its own CPU had
//! just written.
//!
//! Here each landing notes the 256-byte lines in which it found bytes the
//! card wrote. A read of lines in which no landing found any for a while is
//! made at once. What that can get wrong: the first read of a line that
//! shaders begin to write gets the bytes from before; such reads are found
//! when the flight lands, and counted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

const LINE: usize = 256;
const CHUNK: usize = 1 << 20;
/// How long a line counts as one shaders write.
const KEPT_MS: u32 = 5000;

struct Chunk {
    /// When a landing last found bytes the card wrote (milliseconds, 0:
    /// never).
    wrote: Box<[u32; CHUNK / LINE]>,
    /// The last submission started when the line was last read ahead of
    /// flights (0: never).
    read: Box<[u32; CHUNK / LINE]>,
}

#[derive(Default)]
struct Lines(HashMap<usize, Chunk>);

impl Lines {
    fn chunk(&mut self, chunk: usize) -> &mut Chunk {
        self.0.entry(chunk).or_insert_with(|| Chunk {
            wrote: Box::new([0; CHUNK / LINE]),
            read: Box::new([0; CHUNK / LINE]),
        })
    }

    /// Notes the lines of `[at, at + now.len())` whose bytes differ from
    /// `held`. How many of them had been read ahead of submission `landing`
    /// or a later one.
    fn wrote(&mut self, at: usize, now: &[u8], held: &[u8], ms: u32, landing: u64) -> u64 {
        let (mut missed, mut from) = (0, 0);
        while from < now.len() {
            let line = at + from;
            let to = (from + LINE - line % LINE).min(now.len());
            if now[from..to] != held[from..to] {
                let chunk = self.chunk(line / CHUNK);
                let i = line % CHUNK / LINE;
                chunk.wrote[i] = ms.max(1);
                let read = std::mem::take(&mut chunk.read[i]);
                missed += u64::from(read != 0 && u64::from(read) >= landing);
            }
            from = to;
        }
        missed
    }

    /// Whether shaders write in `[at, at + len)`; if not, notes that it is
    /// read now, after submission `started`.
    fn written(&mut self, at: usize, len: usize, ms: u32, started: u64) -> bool {
        let lines = at / LINE..(at + len).div_ceil(LINE);
        let written = lines.clone().any(|line| {
            self.0.get(&(line * LINE / CHUNK)).is_some_and(|c| {
                let wrote = c.wrote[line % (CHUNK / LINE)];
                wrote != 0 && ms.saturating_sub(wrote) < KEPT_MS
            })
        });
        if !written {
            for line in lines {
                self.chunk(line * LINE / CHUNK).read[line % (CHUNK / LINE)] = started as u32;
            }
        }
        written
    }
}

static LINES: Mutex<Option<Lines>> = Mutex::new(None);
/// Reads made ahead of flights, and lines of them found written by one.
static READS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// Whether reads wait only for what shaders were seen to write.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("VICTORY_READ_LEARN").is_ok_and(|v| v == "1"))
}

/// A landing of flights, `landing` the first of them, found `now` where it
/// held `held`, for title memory at `at`.
pub(crate) fn wrote(at: usize, now: &[u8], held: &[u8], landing: u64) {
    if on() {
        let ms = crate::host::now_ms() as u32;
        let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
        READS[1].fetch_add(lines.get_or_insert_default().wrote(at, now, held, ms, landing), Relaxed);
    }
}

/// Whether a read of title memory `[at, at + len)`, which flights' shaders
/// declare, has to wait for them; `started` is the last submission started.
pub(crate) fn written(at: usize, len: usize, started: u64) -> bool {
    if !on() {
        return true;
    }
    let ms = crate::host::now_ms() as u32;
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    let written = lines.get_or_insert_default().written(at, len, ms, started);
    READS[0].fetch_add(u64::from(!written), Relaxed);
    written
}

/// [`READS`], taken.
pub(crate) fn taken() -> [u64; 2] {
    [0, 1].map(|i| READS[i].swap(0, Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_waits_for_lines_the_card_was_seen_to_write() {
        let mut lines = Lines::default();
        let at = 3 * CHUNK - LINE - 8;
        // Nothing seen: read at once, over a chunk's end too.
        assert!(!lines.written(at, 2 * LINE, 100, 7));
        let held = vec![0u8; 3 * LINE];
        let mut now = held.clone();
        now[LINE + 20] = 1;
        // The line at `at + LINE + 20` had been read after submission 7
        // started: too early for flight 7, not for flight 8.
        assert_eq!(lines.wrote(at, &now, &held, 200, 7), 1);
        assert!(lines.written(at + LINE + 8, 1, 300, 9));
        assert!(!lines.written(at + 8, 1, 300, 9));
        assert!(!lines.written(at + LINE + 8, 1, 200 + KEPT_MS, 9));
        assert_eq!(lines.wrote(at, &now, &held, 6000, 10), 0);
        assert!(lines.written(at + LINE + 8, 1, 6001, 11));
    }
}
