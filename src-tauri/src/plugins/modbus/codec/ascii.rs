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
    if frame.len() < 9 || frame.first() != Some(&b':') || !frame.ends_with(b"\r\n") {
        return Err("invalid ASCII frame delimiters".into());
    }
    let hex = &frame[1..frame.len() - 2];
    if hex.len() % 2 != 0 {
        return Err("ASCII frame contains odd hex digit count".into());
    }
    let mut raw = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
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
}
