pub const MBAP_LEN: usize = 7;

pub fn encode(transaction_id: u16, unit_id: u8, pdu: &[u8]) -> Result<Vec<u8>, String> {
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid TCP PDU length".into());
    }
    let length = pdu.len() + 1;
    let mut out = Vec::with_capacity(MBAP_LEN + pdu.len());
    out.extend_from_slice(&transaction_id.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.push(unit_id);
    out.extend_from_slice(pdu);
    Ok(out)
}

pub fn decode(frame: &[u8]) -> Result<(u16, u8, Vec<u8>), String> {
    if frame.len() < MBAP_LEN + 1 {
        return Err("TCP ADU too short".into());
    }
    let transaction_id = u16::from_be_bytes([frame[0], frame[1]]);
    let protocol_id = u16::from_be_bytes([frame[2], frame[3]]);
    if protocol_id != 0 {
        return Err(format!("invalid Protocol Identifier {protocol_id}"));
    }
    let length = u16::from_be_bytes([frame[4], frame[5]]) as usize;
    if !(2..=254).contains(&length) {
        return Err(format!("invalid MBAP length {length}"));
    }
    if frame.len() != 6 + length {
        return Err(format!(
            "MBAP length mismatch: header={length}, actual={}",
            frame.len() - 6
        ));
    }
    let pdu = frame[7..].to_vec();
    if pdu.is_empty() || pdu.len() > super::pdu::MAX_PDU_LEN {
        return Err("invalid TCP PDU length".into());
    }
    Ok((transaction_id, frame[6], pdu))
}

#[derive(Default)]
pub struct TcpFramer {
    buffer: Vec<u8>,
}

impl TcpFramer {
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();
        loop {
            if self.buffer.len() < MBAP_LEN {
                break;
            }
            let protocol_id = u16::from_be_bytes([self.buffer[2], self.buffer[3]]);
            if protocol_id != 0 {
                return Err(format!("invalid Protocol Identifier {protocol_id}"));
            }
            let length = u16::from_be_bytes([self.buffer[4], self.buffer[5]]) as usize;
            if !(2..=254).contains(&length) {
                return Err(format!("invalid MBAP length {length}"));
            }
            let total = 6 + length;
            if self.buffer.len() < total {
                break;
            }
            frames.push(self.buffer.drain(..total).collect());
        }
        Ok(frames)
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_and_coalesced_frames_are_split() {
        let a = encode(1, 1, &[3, 0, 0, 0, 1]).unwrap();
        let b = encode(2, 1, &[4, 0, 0, 0, 1]).unwrap();
        let mut framer = TcpFramer::default();
        assert!(framer.push(&a[..4]).unwrap().is_empty());
        let mut tail = a[4..].to_vec();
        tail.extend_from_slice(&b);
        let frames = framer.push(&tail).unwrap();
        assert_eq!(frames, vec![a, b]);
        assert_eq!(framer.buffered_len(), 0);
    }
}
