use aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, Nonce as AesNonce};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use crypto_common::KeyInit;

use crate::{Error, Result};

const TAG_LEN: usize = 16;
const NONCE_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoMode {
    Aes256Gcm,
    XChaCha20Poly1305,
}

impl CryptoMode {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "aead_aes256_gcm_rtpsize" => Some(Self::Aes256Gcm),
            "aead_xchacha20_poly1305_rtpsize" => Some(Self::XChaCha20Poly1305),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Aes256Gcm => "aead_aes256_gcm_rtpsize",
            Self::XChaCha20Poly1305 => "aead_xchacha20_poly1305_rtpsize",
        }
    }

    pub fn negotiate(modes: &[String]) -> Result<Self> {
        let mut found = None;
        for name in modes {
            match Self::from_name(name) {
                Some(Self::Aes256Gcm) => return Ok(Self::Aes256Gcm),
                Some(mode) => found = Some(mode),
                None => {}
            }
        }
        found.ok_or_else(|| Error::Crypto("no supported encryption mode".to_string()))
    }
}

enum Cipher {
    Aes(Box<Aes256Gcm>),
    XChaCha(XChaCha20Poly1305),
}

pub struct TransportCrypto {
    cipher: Cipher,
    mode: CryptoMode,
    counter: u32,
}

impl TransportCrypto {
    pub fn new(mode: CryptoMode, key: &[u8]) -> Result<Self> {
        let cipher = match mode {
            CryptoMode::Aes256Gcm => Aes256Gcm::new_from_slice(key)
                .map(Box::new)
                .map(Cipher::Aes)
                .map_err(|_| Error::Crypto("invalid aes key".to_string()))?,
            CryptoMode::XChaCha20Poly1305 => XChaCha20Poly1305::new_from_slice(key)
                .map(Cipher::XChaCha)
                .map_err(|_| Error::Crypto("invalid xchacha key".to_string()))?,
        };
        Ok(Self {
            cipher,
            mode,
            counter: rand::random(),
        })
    }

    #[allow(dead_code)] // used by the audio engine
    pub fn encrypt(&mut self, header: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
        let nonce_value = self.counter;
        self.counter = self.counter.wrapping_add(1);

        let mut out = Vec::with_capacity(header.len() + payload.len() + TAG_LEN + NONCE_LEN);
        out.extend_from_slice(header);
        out.extend_from_slice(payload);
        out.extend_from_slice(&[0u8; TAG_LEN]);
        out.extend_from_slice(&nonce_value.to_be_bytes());

        let body = &mut out[header.len()..];
        let (plain, tail) = body.split_at_mut(payload.len());
        let (tag, nonce) = tail.split_at_mut(TAG_LEN);

        match &self.cipher {
            Cipher::Aes(cipher) => {
                let mut nonce_array = AesNonce::default();
                nonce_array[..NONCE_LEN].copy_from_slice(nonce);
                let computed = cipher
                    .encrypt_in_place_detached(&nonce_array, header, plain)
                    .map_err(|_| Error::Crypto("aes encrypt failed".to_string()))?;
                tag.copy_from_slice(&computed[..]);
            }
            Cipher::XChaCha(cipher) => {
                let mut nonce_array = XNonce::default();
                nonce_array[..NONCE_LEN].copy_from_slice(nonce);
                let computed = cipher
                    .encrypt_in_place_detached(&nonce_array, header, plain)
                    .map_err(|_| Error::Crypto("xchacha encrypt failed".to_string()))?;
                tag.copy_from_slice(&computed[..]);
            }
        }

        Ok(out)
    }

    pub fn decrypt(&self, packet: &[u8], header_len: usize) -> Result<Vec<u8>> {
        if packet.len() < header_len + TAG_LEN + NONCE_LEN {
            return Err(Error::Crypto("packet too small".to_string()));
        }
        let (header, rest) = packet.split_at(header_len);
        let (tagged, nonce) = rest.split_at(rest.len() - NONCE_LEN);
        let (body, tag) = tagged.split_at(tagged.len() - TAG_LEN);
        let mut plain = body.to_vec();

        match &self.cipher {
            Cipher::Aes(cipher) => {
                let mut nonce_array = AesNonce::default();
                nonce_array[..NONCE_LEN].copy_from_slice(nonce);
                let tag = aead::generic_array::GenericArray::from_slice(tag);
                cipher
                    .decrypt_in_place_detached(&nonce_array, header, &mut plain, tag)
                    .map_err(|_| Error::Crypto("aes decrypt failed".to_string()))?;
            }
            Cipher::XChaCha(cipher) => {
                let mut nonce_array = XNonce::default();
                nonce_array[..NONCE_LEN].copy_from_slice(nonce);
                let tag = aead::generic_array::GenericArray::from_slice(tag);
                cipher
                    .decrypt_in_place_detached(&nonce_array, header, &mut plain, tag)
                    .map_err(|_| Error::Crypto("xchacha decrypt failed".to_string()))?;
            }
        }

        let mut out = Vec::with_capacity(header.len() + plain.len());
        out.extend_from_slice(header);
        out.extend_from_slice(&plain);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(mode: CryptoMode) {
        let key = [7u8; 32];
        let header = [0x80, 120, 0, 1, 0, 0, 3, 0xc0, 1, 2, 3, 4];
        let payload = b"opus frame payload";
        let mut sender = TransportCrypto::new(mode, &key).expect("cipher");
        let encrypted = sender.encrypt(&header, payload).expect("encrypt");
        assert_eq!(encrypted.len(), header.len() + payload.len() + 20);
        assert_ne!(
            &encrypted[header.len()..header.len() + payload.len()],
            payload
        );
        let receiver = TransportCrypto::new(mode, &key).expect("cipher");
        let decrypted = receiver.decrypt(&encrypted, header.len()).expect("decrypt");
        assert_eq!(&decrypted[..header.len()], &header);
        assert_eq!(&decrypted[header.len()..], payload);
    }

    #[test]
    fn encrypts_and_decrypts_both_modes() {
        round_trip(CryptoMode::Aes256Gcm);
        round_trip(CryptoMode::XChaCha20Poly1305);
    }

    #[test]
    fn rejects_tampering() {
        let key = [9u8; 32];
        let header = [0u8; 12];
        let mut sender = TransportCrypto::new(CryptoMode::XChaCha20Poly1305, &key).expect("cipher");
        let mut encrypted = sender.encrypt(&header, b"hello world").expect("encrypt");
        let last = encrypted.len() - 1;
        encrypted[last] ^= 1;
        let receiver = TransportCrypto::new(CryptoMode::XChaCha20Poly1305, &key).expect("cipher");
        assert!(receiver.decrypt(&encrypted, header.len()).is_err());
    }

    #[test]
    fn negotiates_preferred_and_fallback() {
        let modes = vec![
            "aead_xchacha20_poly1305_rtpsize".to_string(),
            "aead_aes256_gcm_rtpsize".to_string(),
        ];
        assert_eq!(
            CryptoMode::negotiate(&modes).unwrap(),
            CryptoMode::Aes256Gcm
        );
        let modes = vec!["aead_xchacha20_poly1305_rtpsize".to_string()];
        assert_eq!(
            CryptoMode::negotiate(&modes).unwrap(),
            CryptoMode::XChaCha20Poly1305
        );
        assert!(CryptoMode::negotiate(&["xsalsa20_poly1305".to_string()]).is_err());
    }
}
