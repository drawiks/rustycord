const REQUEST_TYPE: u16 = 1;
const RESPONSE_TYPE: u16 = 2;
const DISCOVERY_LEN: usize = 74;
const DISCOVERY_PAYLOAD_LEN: u16 = 70;
const PING_REQUEST: u32 = 0x1337_cafe;
const PING_RESPONSE: u32 = 0x1337_f00d;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResponse {
    pub address: String,
    pub port: u16,
}

pub fn discovery_request(ssrc: u32) -> [u8; DISCOVERY_LEN] {
    let mut packet = [0u8; DISCOVERY_LEN];
    packet[0..2].copy_from_slice(&REQUEST_TYPE.to_be_bytes());
    packet[2..4].copy_from_slice(&DISCOVERY_PAYLOAD_LEN.to_be_bytes());
    packet[4..8].copy_from_slice(&ssrc.to_be_bytes());
    packet
}

pub fn parse_discovery(data: &[u8]) -> Option<DiscoveryResponse> {
    if data.len() < DISCOVERY_LEN || u16::from_be_bytes([data[0], data[1]]) != RESPONSE_TYPE {
        return None;
    }
    let address_end = data[8..72].iter().position(|byte| *byte == 0)? + 8;
    let address = std::str::from_utf8(&data[8..address_end]).ok()?.to_string();
    let port = u16::from_be_bytes([data[72], data[73]]);
    Some(DiscoveryResponse { address, port })
}

pub fn ping_request(sequence: u32) -> [u8; 8] {
    let mut packet = [0u8; 8];
    packet[0..4].copy_from_slice(&PING_REQUEST.to_be_bytes());
    packet[4..8].copy_from_slice(&sequence.to_be_bytes());
    packet
}

pub fn parse_ping(data: &[u8]) -> Option<u32> {
    if data.len() != 8 || u32::from_be_bytes([data[0], data[1], data[2], data[3]]) != PING_RESPONSE {
        return None;
    }
    Some(u32::from_be_bytes([data[4], data[5], data[6], data[7]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_parses_discovery() {
        let request = discovery_request(0x0102_0304);
        assert_eq!(u16::from_be_bytes([request[0], request[1]]), REQUEST_TYPE);
        assert_eq!(u16::from_be_bytes([request[2], request[3]]), 70);
        assert_eq!(&request[4..8], &[1, 2, 3, 4]);

        let mut response = [0u8; DISCOVERY_LEN];
        response[0..2].copy_from_slice(&RESPONSE_TYPE.to_be_bytes());
        response[8..17].copy_from_slice(b"127.0.0.1");
        response[72..74].copy_from_slice(&50000u16.to_be_bytes());
        let parsed = parse_discovery(&response).expect("discovery");
        assert_eq!(parsed.address, "127.0.0.1");
        assert_eq!(parsed.port, 50000);
    }

    #[test]
    fn rejects_bad_discovery() {
        assert!(parse_discovery(&[]).is_none());
        assert!(parse_discovery(&[0u8; DISCOVERY_LEN]).is_none());
    }

    #[test]
    fn pings() {
        let request = ping_request(7);
        assert_eq!(&request[4..], &7u32.to_be_bytes());
        let mut response = [0u8; 8];
        response[0..4].copy_from_slice(&PING_RESPONSE.to_be_bytes());
        response[4..8].copy_from_slice(&7u32.to_be_bytes());
        assert_eq!(parse_ping(&response), Some(7));
        assert_eq!(parse_ping(&[0u8; 8]), None);
    }
}
