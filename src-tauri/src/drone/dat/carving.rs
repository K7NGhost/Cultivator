//! Bounded, read-only signature scanning and contiguous DJI DAT recovery.
//! Offsets address decoded image bytes, including for compressed/split containers.
use super::{header, packet::Scanner};
use inodex::image::DiskImage;
use sha2::{Digest, Sha256};

const BLOCK: usize = 8 * 1024 * 1024;
const PROBE: usize = 64 * 1024;
const MAX_GAP: usize = 64 * 1024;
const MIN_PACKETS: usize = 3;
const MAX_CANDIDATES: usize = 10_000;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub offset: u64,
    pub format: String,
}

pub struct Recovered {
    pub bytes: Vec<u8>,
    pub fingerprint: String,
    pub stopped_at_size_limit: bool,
}

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("DAT carving cancelled".into())
    } else {
        Ok(())
    }
}

pub fn scan(
    image: &mut dyn DiskImage,
    cancelled: &dyn Fn() -> bool,
    mut progress: impl FnMut(u64, u64, usize),
) -> Result<Vec<Candidate>, String> {
    let needle = memchr::memmem::Finder::new(b"BUILD ");
    let length = image.len();
    let mut candidates = Vec::new();
    let mut offset = 0u64;
    let mut bytes = vec![0; BLOCK + 272];
    let mut previous = 0;
    progress(0, length, 0);
    while offset < length {
        checkpoint(cancelled)?;
        let size = (length - offset).min(BLOCK as u64) as usize;
        let base = offset - previous as u64;
        let end = previous + size;
        image
            .read_exact_at(offset, &mut bytes[previous..end])
            .map_err(|e| e.to_string())?;
        for found in needle.find_iter(&bytes[..end]) {
            checkpoint(cancelled)?;
            let Some(start) = found.checked_sub(16) else {
                continue;
            };
            // Defer a header straddling the buffer boundary to the next block.
            if end - start < 256 && offset + (size as u64) < length {
                continue;
            }
            let absolute = base + start as u64;
            if candidates
                .last()
                .is_some_and(|last: &Candidate| last.offset >= absolute)
            {
                continue;
            }
            let Ok((record_start, format, _, _)) = header(&bytes[start..end]) else {
                continue;
            };
            let available = (length - absolute).min(PROBE as u64) as usize;
            let mut probe = Vec::new();
            let probe = if end - start >= available {
                &bytes[start..start + available]
            } else {
                probe.resize(available, 0);
                image
                    .read_exact_at(absolute, &mut probe)
                    .map_err(|e| e.to_string())?;
                &probe
            };
            let mut records = Scanner::new(probe, record_start);
            let Some(first) = records.next() else {
                continue;
            };
            if first.offset - record_start > 4096
                || records.take(MIN_PACKETS - 1).count() < MIN_PACKETS - 1
            {
                continue;
            }
            if candidates.len() >= MAX_CANDIDATES {
                return Err("DAT carve candidate limit exceeded (10,000)".into());
            }
            candidates.push(Candidate {
                offset: absolute,
                format: format.into(),
            });
        }
        offset += size as u64;
        previous = end.min(272);
        bytes.copy_within(end - previous..end, 0);
        progress(offset, length, candidates.len());
    }
    Ok(candidates)
}

/// A fingerprint of the header and CRC-valid records ignores filesystem padding.
/// It compares complete validated record streams, never just header prefixes.
pub fn fingerprint(bytes: &[u8], cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    let (start, _, _, _) = header(bytes)?;
    let mut digest = Sha256::new();
    let mut span_start = 0;
    let mut span_end = start;
    for (count, packet) in Scanner::new(bytes, start).enumerate() {
        if count % 4096 == 0 {
            checkpoint(cancelled)?;
        }
        if packet.offset != span_end {
            digest.update(&bytes[span_start..span_end]);
            span_start = packet.offset;
        }
        span_end = packet.offset + packet.encrypted.len() + 12;
    }
    digest.update(&bytes[span_start..span_end]);
    Ok(format!("{:x}", digest.finalize()))
}

pub fn recover(
    image: &mut dyn DiskImage,
    candidate: &Candidate,
    next_header: u64,
    max_bytes: u64,
    cancelled: &dyn Fn() -> bool,
    mut progress: impl FnMut(u64),
) -> Result<Recovered, String> {
    let available = next_header
        .min(image.len())
        .saturating_sub(candidate.offset);
    let limit = available.min(max_bytes).min(super::MAX_SOURCE_BYTES);
    if limit < 256 {
        return Err("Truncated DAT carve candidate".into());
    }
    let mut bytes = Vec::new();
    let mut last_good = 0usize;
    let mut packets = 0usize;
    let mut stopped_for_gap = false;
    let mut digest = Sha256::new();
    while (bytes.len() as u64) < limit {
        checkpoint(cancelled)?;
        let previous = bytes.len();
        let size = (limit - previous as u64).min(1024 * 1024) as usize;
        bytes.resize(previous + size, 0);
        image
            .read_exact_at(candidate.offset + previous as u64, &mut bytes[previous..])
            .map_err(|e| e.to_string())?;
        if last_good == 0 {
            last_good = header(&bytes)?.0;
            digest.update(&bytes[..last_good]);
        }
        let mut span_start = last_good;
        for packet in Scanner::new(&bytes, last_good) {
            if packet.offset - last_good > MAX_GAP {
                stopped_for_gap = true;
                break;
            }
            if packet.offset != last_good {
                digest.update(&bytes[span_start..last_good]);
                span_start = packet.offset;
            }
            last_good = packet.offset + packet.encrypted.len() + 12;
            packets += 1;
            if packets % 4096 == 0 {
                checkpoint(cancelled)?;
            }
        }
        digest.update(&bytes[span_start..last_good]);
        progress(bytes.len() as u64);
        if stopped_for_gap || bytes.len() - last_good > MAX_GAP {
            stopped_for_gap = true;
            break;
        }
    }
    if packets < MIN_PACKETS {
        return Err("Not enough contiguous CRC-valid DAT packets to recover".into());
    }
    let stopped_at_size_limit = !stopped_for_gap && available > limit;
    bytes.truncate(last_good);
    let fingerprint = format!("{:x}", digest.finalize());
    Ok(Recovered {
        bytes,
        fingerprint,
        stopped_at_size_limit,
    })
}

#[cfg(test)]
mod tests;
