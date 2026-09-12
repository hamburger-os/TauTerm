const MAX_ASCII_ADU_LEN: usize = (super::pdu::MAX_PDU_LEN + 2) * 2 + 3;

pub fn lrc(data: &[u8]) -> u8 {
    (!data.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))).wrapping_add(1)
}

pub fn encode(unit_id: u8, pdu: &[u8]) -> Result<Vec<u8>, String> {
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid ASCII PDU length".into());
    }
    let mut raw = Vec::with_capacity(pdu.len() + 2);
    raw.push(unit_id);
    raw.extend_from_slice(pdu);
    raw.push(lrc(&raw));
    let mut out = Vec::with_capacity(raw.len() * 2 + 3);
    out.push(b':');
    for byte in raw {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    Ok(out)
}

pub fn decode(frame: &[u8]) -> Result<(u8, Vec<u8>), String> {
    if frame.len() < 9 || frame.len() > MAX_ASCII_ADU_LEN {
        return Err("invalid ASCII frame length".into());
    }
    if frame.first() != Some(&b':') || !frame.ends_with(b"\r\n") {
        return Err("invalid ASCII frame delimiters".into());
    }
    let hex = &frame[1..frame.len() - 2];
    if !hex.len().is_multiple_of(2) {
        return Err("ASCII frame contains odd hex digit count".into());
    }
    let mut raw = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_chunks::<2>().0 {
        raw.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    if raw.len() < 3 {
        return Err("ASCII frame payload too short".into());
    }
    let actual = raw.pop().expect("length checked");
    let expected = lrc(&raw);
    if actual != expected {
        return Err(format!(
            "LRC mismatch: expected 0x{expected:02X}, got 0x{actual:02X}"
        ));
    }
    let unit = raw[0];
    let pdu = raw[1..].to_vec();
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid ASCII PDU length".into());
    }
    Ok((unit, pdu))
}

#[derive(Default)]
pub struct AsciiFramer {
    buffer: Vec<u8>,
}

impl AsciiFramer {
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();

        loop {
            let Some(start) = self.buffer.iter().position(|byte| *byte == b':') else {
                self.buffer.clear();
                break;
            };
            if start > 0 {
                self.buffer.drain(..start);
            }

            if let Some(next_start) = self.buffer[1..].iter().position(|byte| *byte == b':') {
                let next_start = next_start + 1;
                let end_before_next = self.buffer[..next_start]
                    .windows(2)
                    .position(|window| window == b"\r\n");
                if end_before_next.is_none() {
                    self.buffer.drain(..next_start);
                    continue;
                }
            }

            let Some(end) = self
                .buffer
                .windows(2)
                .position(|window| window == b"\r\n")
            else {
                if self.buffer.len() > MAX_ASCII_ADU_LEN {
                    self.buffer.clear();
                }
                break;
            };
            let frame_len = end + 2;
            if frame_len > MAX_ASCII_ADU_LEN {
                self.buffer.drain(..frame_len);
                continue;
            }
            frames.push(self.buffer.drain(..frame_len).collect());
        }

        frames
    }

    #[cfg(test)]
    fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

fn hex_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(format!("invalid ASCII hex character 0x{value:02X}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trip_and_lrc_validation() {
        let frame = encode(1, &[0x03, 0, 0, 0, 1]).unwrap();
        assert_eq!(decode(&frame).unwrap(), (1, vec![0x03, 0, 0, 0, 1]));
        let mut broken = frame;
        broken[3] = b'F';
        assert!(decode(&broken).is_err());
    }

    #[test]
    fn framer_handles_fragmentation_coalescing_and_noise() {
        let a = encode(1, &[0x03, 0, 0, 0, 1]).unwrap();
        let b = encode(2, &[0x04, 0, 0, 0, 1]).unwrap();
        let mut framer = AsciiFramer::default();
        assert!(framer.push(b"noise").is_empty());
        assert_eq!(framer.buffered_len(), 0);
        assert!(framer.push(&a[..4]).is_empty());
        let mut tail = a[4..].to_vec();
        tail.extend_from_slice(&b);
        assert_eq!(framer.push(&tail), vec![a, b]);
        assert_eq!(framer.buffered_len(), 0);
    }

    #[test]
    fn framer_resynchronizes_on_new_start_delimiter() {
        let good = encode(1, &[0x03, 0, 0, 0, 1]).unwrap();
        let mut chunk = b":0103DEAD".to_vec();
        chunk.extend_from_slice(&good);
        let mut framer = AsciiFramer::default();
        assert_eq!(framer.push(&chunk), vec![good]);
    }

    #[test]
    fn framer_bounds_unterminated_input() {
        let mut framer = AsciiFramer::default();
        let mut oversized = vec![b':'];
        oversized.extend(std::iter::repeat_n(b'0', MAX_ASCII_ADU_LEN + 10));
        assert!(framer.push(&oversized).is_empty());
        assert_eq!(framer.buffered_len(), 0);
    }
}
