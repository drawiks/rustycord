pub const OPUS_PAYLOAD_TYPE: u8 = 120;
pub const HEADER_LEN: usize = 12;

const RTP_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
    pub payload_type: u8,
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
}

impl RtpHeader {
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < HEADER_LEN || data[0] >> 6 != RTP_VERSION {
            return None;
        }
        let csrc_count = (data[0] & 0x0f) as usize;
        let mut offset = HEADER_LEN + csrc_count * 4;
        if data.len() < offset {
            return None;
        }
        if data[0] & 0x10 != 0 {
            if data.len() < offset + 4 {
                return None;
            }
            let words = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4 + words * 4;
            if data.len() < offset {
                return None;
            }
        }
        Some((
            Self {
                payload_type: data[1] & 0x7f,
                sequence: u16::from_be_bytes([data[2], data[3]]),
                timestamp: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
                ssrc: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            },
            offset,
        ))
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.push(0x80);
        out.push(self.payload_type);
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.extend_from_slice(&self.ssrc.to_be_bytes());
    }
}

pub fn build(header: &RtpHeader, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    header.write(&mut out);
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_header() {
        let header = RtpHeader {
            payload_type: OPUS_PAYLOAD_TYPE,
            sequence: 42,
            timestamp: 960,
            ssrc: 0xdead_beef,
        };
        let packet = build(&header, &[1, 2, 3]);
        let (parsed, offset) = RtpHeader::parse(&packet).expect("header");
        assert_eq!(parsed, header);
        assert_eq!(offset, HEADER_LEN);
        assert_eq!(&packet[offset..], &[1, 2, 3]);
    }

    #[test]
    fn rejects_short_and_wrong_version() {
        assert!(RtpHeader::parse(&[0x80; 11]).is_none());
        let mut packet = [0u8; HEADER_LEN];
        packet[0] = 0x40;
        assert!(RtpHeader::parse(&packet).is_none());
    }

    #[test]
    fn skips_csrc_and_extension() {
        let mut packet = vec![0u8; HEADER_LEN];
        packet[0] = 0x90;
        packet[2] = 0;
        packet[3] = 7;
        packet.extend_from_slice(&[0; 4]);
        packet.extend_from_slice(&[0xbe, 0xde, 0, 1]);
        packet.extend_from_slice(&[0; 4]);
        packet.extend_from_slice(&[9, 9]);
        let (header, offset) = RtpHeader::parse(&packet).expect("header");
        assert_eq!(header.sequence, 7);
        assert_eq!(offset, HEADER_LEN + 12);
        assert_eq!(&packet[offset..], &[9, 9]);
    }
}
