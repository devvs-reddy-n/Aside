//! A bounded replay queue. Audio stays until its *final transcript timing*
//! confirms it; an AudioAdded acknowledgement alone cannot prove recognition.
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct Packet {
    pub id: u64,
    pub bytes: Vec<u8>,
    pub trimmed: usize,
}

pub struct AudioBuffer {
    packets: VecDeque<Packet>,
    next_id: u64,
    bytes: usize,
    max_bytes: usize,
    /// Only evicted audio that contained sound. Long silence pushed out of
    /// the replay window is normal and must never be reported as lost speech.
    pub dropped_bytes: usize,
}

/// Quieter than about -40 dBFS RMS counts as silence for loss reporting.
const LOSS_RMS_FLOOR: f64 = 0.01 * 32768.0;

fn has_sound(bytes: &[u8]) -> bool {
    let samples = bytes.len() / 2;
    if samples == 0 { return false; }
    let energy: f64 = bytes.chunks_exact(2)
        .map(|b| { let v = i16::from_le_bytes([b[0], b[1]]) as f64; v * v })
        .sum();
    (energy / samples as f64).sqrt() > LOSS_RMS_FLOOR
}

impl AudioBuffer {
    pub fn new(rate: u32) -> Self {
        Self { packets: VecDeque::new(), next_id: 1, bytes: 0,
            max_bytes: rate as usize * 2 * 20, dropped_bytes: 0 }
    }

    pub fn push(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() { return; }
        self.bytes += bytes.len();
        self.packets.push_back(Packet { id: self.next_id, bytes, trimmed: 0 });
        self.next_id += 1;
        while self.bytes > self.max_bytes {
            if let Some(old) = self.packets.pop_front() {
                self.bytes -= old.bytes.len();
                if has_sound(&old.bytes) { self.dropped_bytes += old.bytes.len(); }
            }
        }
    }

    pub fn next_after(&self, id: u64) -> Option<Packet> {
        self.packets.iter().find(|p| p.id > id).cloned()
    }

    /// `through` is an offset in the packet as originally captured. This is
    /// idempotent even when successive finals split the same packet.
    pub fn confirm(&mut self, id: u64, through: usize) {
        while self.packets.front().is_some_and(|p| p.id < id) {
            let old = self.packets.pop_front().unwrap();
            self.bytes -= old.bytes.len();
        }
        if let Some(packet) = self.packets.front_mut().filter(|p| p.id == id) {
            let remove = through.saturating_sub(packet.trimmed).min(packet.bytes.len()) / 2 * 2;
            packet.bytes.drain(..remove);
            packet.trimmed += remove;
            self.bytes -= remove;
            if packet.bytes.is_empty() { self.packets.pop_front(); }
        }
    }
}

/// A connection's wire clock starts at zero, including after reconnecting.
/// This map translates provider timestamps back into captured packet offsets.
#[derive(Default)]
pub struct SentAudio {
    frames: VecDeque<(u64, usize, usize, u64)>,
    samples: u64,
}

impl SentAudio {
    pub fn sent(&mut self, packet: &Packet) {
        self.samples += (packet.bytes.len() / 2) as u64;
        self.frames.push_back((packet.id, packet.trimmed, packet.bytes.len(), self.samples));
    }

    pub fn confirm(&mut self, buffer: &mut AudioBuffer, seconds: f64, rate: u32) {
        if !seconds.is_finite() || seconds < 0.0 { return; }
        let cutoff = ((seconds * rate as f64).round() as u64).min(self.samples);
        while let Some(&(id, trimmed, len, end)) = self.frames.front() {
            let start = end - (len / 2) as u64;
            if cutoff <= start { break; }
            let count = ((cutoff - start) * 2).min(len as u64) as usize;
            buffer.confirm(id, trimmed + count);
            if cutoff < end { break; }
            self.frames.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_replays_only_unconfirmed_audio_in_order() {
        let mut buffer = AudioBuffer::new(16_000);
        buffer.push(vec![1; 3200]); buffer.push(vec![2; 3200]);
        let mut sent = SentAudio::default();
        sent.sent(&buffer.next_after(0).unwrap());
        sent.sent(&buffer.next_after(1).unwrap());
        sent.confirm(&mut buffer, 0.15, 16_000);
        let remaining = buffer.next_after(0).unwrap();
        assert_eq!(remaining.id, 2);
        assert_eq!(remaining.bytes, vec![2;1600]);
        // A replacement session confirms its remaining 50ms on its own clock.
        let mut reconnect = SentAudio::default(); reconnect.sent(&remaining);
        reconnect.confirm(&mut buffer, 0.05, 16_000);
        assert!(buffer.next_after(0).is_none());
    }

    #[test]
    fn repeated_final_timing_does_not_trim_twice() {
        let mut buffer = AudioBuffer::new(16_000); buffer.push(vec![1;3200]);
        let mut sent = SentAudio::default(); sent.sent(&buffer.next_after(0).unwrap());
        for _ in 0..3 { sent.confirm(&mut buffer, 0.05, 16_000); }
        assert_eq!(buffer.next_after(0).unwrap().bytes.len(),1600);
    }

    #[test]
    fn long_outages_are_memory_bounded_and_count_lost_audio() {
        let loud: Vec<u8> = (0..640).flat_map(|i| (if i % 2 == 0 { 8000i16 } else { -8000 }).to_le_bytes()).collect();
        let mut buffer = AudioBuffer::new(16_000);
        for _ in 0..1000 { buffer.push(loud.clone()); }
        assert_eq!(buffer.bytes,640_000);
        assert_eq!(buffer.dropped_bytes,640_000);
        assert_eq!(buffer.next_after(0).unwrap().id,501);
    }

    #[test]
    fn evicted_silence_is_not_reported_as_lost_speech() {
        // Faint room noise (about -60 dBFS) for 40 s: twice the replay window.
        let hiss: Vec<u8> = (0..640).flat_map(|i| (if i % 2 == 0 { 30i16 } else { -30 }).to_le_bytes()).collect();
        let mut buffer = AudioBuffer::new(16_000);
        for _ in 0..1000 { buffer.push(hiss.clone()); }
        assert_eq!(buffer.bytes,640_000, "still memory-bounded");
        assert_eq!(buffer.dropped_bytes,0);
    }
}
