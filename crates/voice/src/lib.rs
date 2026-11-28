mod audio;
mod crypto;
mod dave;
mod discovery;
mod error;
mod rtp;
mod session;

pub use audio::{AudioEngine, input_devices, output_devices};
pub use crypto::{CryptoMode, TransportCrypto};
pub use discovery::{
    DiscoveryResponse, discovery_request, parse_discovery, parse_ping, ping_request,
};
pub use error::{Error, Result};
pub use rtp::{OPUS_PAYLOAD_TYPE, RtpHeader};
pub use session::{VoiceCommand, VoiceEvent, VoiceHandle, VoiceSession, spawn};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const FRAME_SAMPLES: usize = 960;
