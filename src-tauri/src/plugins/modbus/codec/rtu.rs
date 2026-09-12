pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for byte in data {
        crc ^= *byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn encode(unit_id: u8, pdu: &[u8]) -> Result<Vec<u8>, String> {
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid RTU PDU length".into());
    }
    let mut frame = Vec::with_capacity(pdu.len() + 3);
    frame.push(unit_id);
    frame.extend_from_slice(pdu);
    let crc = crc16(&frame);
    frame.extend_from_slice(&crc.to_le_bytes());
    Ok(frame)
}

pub fn decode(frame: &[u8]) -> Result<(u8, Vec<u8>), String> {
    if frame.len() < 4 {
        return Err("RTU frame too short".into());
    }
    let data_len = frame.len() - 2;
    let expected = crc16(&frame[..data_len]);
    let actual = u16::from_le_bytes([frame[data_len], frame[data_len + 1]]);
    if expected != actual {
        return Err(format!(
            "CRC mismatch: expected 0x{expected:04X}, got 0x{actual:04X}"
        ));
    }
    let pdu = frame[1..data_len].to_vec();
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid RTU PDU length".into());
    }
    Ok((frame[0], pdu))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_crc_vector() {
        // 01 03 00 00 00 0A -> CRC C5CD, transmitted CD C5.
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        assert_eq!(crc16(&data), 0xCDC5);
        let frame = encode(1, &[0x03, 0x00, 0x00, 0x00, 0x0A]).unwrap();
        assert_eq!(&frame[frame.len() - 2..], &[0xC5, 0xCD]);
        assert_eq!(decode(&frame).unwrap(), (1, vec![0x03, 0, 0, 0, 0x0A]));
    }

    #[test]
    fn corrupted_crc_is_rejected() {
        let mut frame = encode(1, &[0x03, 0, 0, 0, 1]).unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert!(decode(&frame).is_err());
    }
}
