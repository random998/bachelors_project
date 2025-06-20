use std::fmt;

/// Cryptographic types and utilities for signing and verifying messages.
use anyhow::{Result, bail};
use bip39::Mnemonic;
use blake2::digest::typenum::ToInt;
use blake2::{Blake2s, Digest, digest};
use ed25519_dalek::{Signer, Verifier};
use rand::rngs::StdRng;
use rand::{CryptoRng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const ENTROPY_LENGTH: usize = 16;
type Entropy = [u8; ENTROPY_LENGTH];

/// Private key used to sign messages.
#[derive(Clone)]
pub struct SigningKey {
    key: ed25519_dalek::SigningKey,
    entropy: Zeroizing<Entropy,>,
}

/// Hasher used to hash messages before signing or verification.
type SignatureHasher = Blake2s<digest::consts::U32,>;

impl Default for SigningKey {
    fn default() -> Self {
        let mut rng = StdRng::from_os_rng();
        Self::generate_from_rng(&mut rng,)
    }
}

impl SigningKey {
    /// Create a signing key from a BIP39 mnemonic phrase.
    pub fn from_phrase(phrase: &str,) -> Result<Self,> {
        let mnemonic = Mnemonic::from_phrase(phrase, Default::default(),)?;
        if mnemonic.entropy().len() != ENTROPY_LENGTH {
            bail!(
                "Invalid entropy length. Expected: {}, got: {}",
                ENTROPY_LENGTH,
                mnemonic.entropy().len()
            );
        }

        let mut entropy = Entropy::default();
        entropy.copy_from_slice(mnemonic.entropy(),);
        Ok(Self::generate_from_entropy(entropy,),)
    }

    /// Sign a serializable message and return the digital signature.
    pub fn sign<T,>(&self, message: &T,) -> Signature
    where T: Serialize {
        let mut hasher = SignatureHasher::new();
        bincode::serialize_into(&mut hasher, message,)
            .expect("Failed to serialize message for signing",);
        Signature(self.key.sign(&hasher.finalize(),),)
    }

    /// Return the mnemonic phrase corresponding to this key's entropy.
    #[must_use]
    pub fn phrase(&self,) -> String {
        Mnemonic::from_entropy(self.entropy.as_ref(), Default::default(),)
            .unwrap()
            .phrase()
            .to_string()
    }

    /// Return the corresponding public key (verifier).
    #[must_use]
    pub fn verifying_key(&self,) -> VerifyingKey {
        VerifyingKey(self.key.verifying_key(),)
    }

    /// Generate a signing key using a provided cryptographically secure RNG.
    fn generate_from_rng<R: RngCore + CryptoRng,>(rng: &mut R,) -> Self {
        let mut entropy = Entropy::default();
        rng.fill_bytes(&mut entropy,);
        Self::generate_from_entropy(entropy,)
    }

    /// Generate a signing key deterministically from a fixed entropy.
    fn generate_from_entropy(entropy: Entropy,) -> Self {
        let key_hash = SignatureHasher::digest(entropy,);
        let key = ed25519_dalek::SigningKey::from_bytes(&key_hash.into(),);
        let entropy = Zeroizing::new(entropy,);
        Self {
            key,
            entropy,
        }
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "SigningKey {}", bs58::encode(self.phrase()).into_string())
    }
}

/// A digital signature.
#[derive(Clone, Copy, Serialize, Deserialize,)]
pub struct Signature(ed25519_dalek::Signature,);

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "Signature {}", bs58::encode(self.0.to_bytes()).into_string())
    }
}

/// Public key used to verify message signatures.
#[derive(Clone, Copy, Serialize, Deserialize,)]
pub struct VerifyingKey(ed25519_dalek::VerifyingKey,);

impl VerifyingKey {
    /// Verify that a message was signed by the corresponding private key.
    pub fn verify<T,>(&self, message: &T, signature: &Signature,) -> bool
    where T: Serialize {
        let mut hasher = SignatureHasher::new();
        bincode::serialize_into(&mut hasher, message,)
            .expect("Failed to serialize message for verification",);
        self.0.verify(&hasher.finalize(), &signature.0,).is_ok()
    }

    /// Derive a `PeerId` from the verifying key.
    #[must_use]
    pub fn peer_id(&self,) -> PeerId {
        let mut hasher = Blake2s::<digest::consts::U16,>::new();
        hasher.update(self.0.as_bytes(),);
        PeerId(hasher.finalize().into(),)
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "VerifyingKey({})", bs58::encode(self.0.as_bytes()).into_string())
    }
}

/// A peer identifier derived from a public key.
#[derive(Clone, Copy, Serialize, Deserialize, Hash, Eq, PartialEq,)]
pub struct PeerId([u8; digest::consts::U16::INT],);

impl PeerId {
    /// Return the hex-encoded identifier string.
    #[must_use]
    pub fn digits(&self,) -> String {
        self.0.iter().fold(String::with_capacity(32,), |mut output, b| {
            output.push_str(&format!("{b:02x}"),);
            output
        },)
    }
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "PeerId {}", self.digits())
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{}", self.digits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phrase_round_trip() {
        let sk = SigningKey::default();
        let restored =
            SigningKey::from_phrase(&sk.phrase(),).expect("Failed to recover key from phrase",);
        assert_eq!(sk.key, restored.key);
    }

    #[test]
    fn test_sign_and_verify() {
        #[derive(Serialize,)]
        struct Point {
            x: f32,
            y: f32,
        }

        let msg = Point {
            x: 10.2,
            y: 4.3,
        };
        let sk = SigningKey::default();
        let signature = sk.sign(&msg,);
        let vk = sk.verifying_key();
        assert!(vk.verify(&msg, &signature), "Signature verification failed");
    }
}
