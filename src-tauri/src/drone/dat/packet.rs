const CRC_TABLE: [u16; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = index as u16;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ if value & 1 != 0 { 0x8408 } else { 0 };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0x3692u16;
    for &byte in bytes {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ byte as u16) & 0xff) as usize];
    }
    crc
}

pub struct Packet<'a> {
    pub offset: usize,
    pub id: u16,
    pub tick: u32,
    pub encrypted: &'a [u8],
}

impl Packet<'_> {
    pub fn decode(&self) -> Vec<u8> {
        self.encrypted.iter().map(|b| b ^ self.tick as u8).collect()
    }
}

pub struct Scanner<'a> {
    data: &'a [u8],
    position: usize,
    pub rejected_packets: usize,
    pub skipped_bytes: usize,
}

impl<'a> Scanner<'a> {
    pub fn new(data: &'a [u8], start: usize) -> Self {
        Self {
            data,
            position: start,
            rejected_packets: 0,
            skipped_bytes: 0,
        }
    }
}

impl<'a> Iterator for Scanner<'a> {
    type Item = Packet<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.position < self.data.len() {
            let pos = self.position;
            if self.data[pos] != 0x55 {
                self.position += 1;
                self.skipped_bytes += 1;
                continue;
            }
            let length = self.data.get(pos + 1).copied().unwrap_or(0) as usize;
            let Some(packet) = self
                .data
                .get(pos..pos.saturating_add(length))
                .filter(|p| p.len() >= 12)
            else {
                self.position += 1;
                self.rejected_packets += 1;
                continue;
            };
            let stored = u16::from_le_bytes([packet[length - 2], packet[length - 1]]);
            if packet[2] != 0 || crc16(&packet[..length - 2]) != stored {
                self.position += 1;
                self.rejected_packets += 1;
                continue;
            }
            self.position += length;
            return Some(Packet {
                offset: pos,
                id: u16::from_le_bytes([packet[4], packet[5]]),
                tick: u32::from_le_bytes(packet[6..10].try_into().unwrap()),
                encrypted: &packet[10..length - 2],
            });
        }
        None
    }
}

#[derive(Default)]
pub struct Clock {
    last: u32,
    offset: u64,
    previous_offset: u64,
    rollover_records: usize,
}
impl Clock {
    pub fn unwrap(&mut self, tick: u32) -> u64 {
        if self.rollover_records == 0 && self.last > 4_292_717_296 && tick < 2_225_000 {
            self.previous_offset = self.offset;
            self.offset += 1u64 << 32;
            self.rollover_records = 100;
        }
        let offset = if self.rollover_records > 0 && tick > 4_292_717_296 {
            self.previous_offset
        } else {
            self.offset
        };
        self.rollover_records = self.rollover_records.saturating_sub(1);
        self.last = tick;
        tick as u64 + offset
    }
}

#[cfg(test)]
pub fn fixture_packet(id: u16, tick: u32, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 243);
    let mut packet = vec![0x55, (payload.len() + 12) as u8, 0, 0];
    packet.extend(id.to_le_bytes());
    packet.extend(tick.to_le_bytes());
    packet.extend(payload.iter().map(|byte| byte ^ tick as u8));
    let checksum = crc16(&packet);
    packet.extend(checksum.to_le_bytes());
    packet
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_matches_datcon_table_seed_vector() {
        // Independently calculated using DatCon's table-driven algorithm.
        assert_eq!(crc16(b"123456789"), 0x7109);
    }
    #[test]
    fn table_crc_matches_bitwise_reference_for_all_byte_values() {
        let bytes: Vec<u8> = (0..1024).map(|index| (index % 256) as u8).collect();
        for length in [0, 1, 2, 31, 255, 256, 1024] {
            let mut reference = 0x3692u16;
            for byte in &bytes[..length] {
                reference ^= *byte as u16;
                for _ in 0..8 {
                    reference = (reference >> 1) ^ if reference & 1 != 0 { 0x8408 } else { 0 };
                }
            }
            assert_eq!(crc16(&bytes[..length]), reference);
        }
    }
    #[test]
    fn recovers_after_bad_crc_and_decodes_final_packet() {
        let mut data = fixture_packet(12, 601, &[1, 2, 3]);
        data[10] ^= 1;
        data.extend(fixture_packet(2048, 602, &[4, 5, 6]));
        let mut scanner = Scanner::new(&data, 0);
        let packet = scanner.next().unwrap();
        assert_eq!(packet.id, 2048);
        assert_eq!(packet.decode(), [4, 5, 6]);
        assert!(scanner.next().is_none());
        assert_eq!(scanner.rejected_packets, 1);
    }
    #[test]
    fn handles_wrap_and_late_pre_wrap_record() {
        let mut clock = Clock::default();
        assert_eq!(clock.unwrap(u32::MAX - 1), u32::MAX as u64 - 1);
        assert_eq!(clock.unwrap(3), (1u64 << 32) + 3);
        assert_eq!(clock.unwrap(u32::MAX), u32::MAX as u64);
        assert_eq!(clock.unwrap(4), (1u64 << 32) + 4);
    }
}
