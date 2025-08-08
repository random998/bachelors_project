//! crates/core/src/crypto.rs
//! A *working* rewrite that uses `libp2p-identity` instead of `ed25519-dalek`.

use crate::message::Payload;
use crate::message::SignedMessage;
use anyhow::{bail, Result};
use bip39::{Language, Mnemonic};
use blake2::digest::consts;
use blake2::{Blake2s, Digest};
use libp2p_identity::ed25519;
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::convert::TryInto;
use std::fmt;
use std::fmt::{Debug, Display};
use zeroize::Zeroizing;
// --------------------------------------------------------------------
// Constants & type aliases

const ENTROPY_LEN: usize = 16; // BIP-39 entropy size we use
const SECRET_LEN: usize = 32; // raw Ed25519 secret length

type Entropy = [u8; ENTROPY_LEN];

/// Hash function used before signing / verifying.
type SigHasher = Blake2s<consts::U32,>; // 32-byte Blake2s

// --------------------------------------------------------------------
// SigningKey

#[derive(Clone,)]
pub struct SecretKey {
    kp:      ed25519::Keypair, // libp2p (public+secret)
    entropy: Zeroizing<Entropy,>,
}

// ---------- constructors -------------------------------------------

impl Default for SecretKey {
    fn default() -> Self {
        let mut rng = StdRng::from_os_rng();
        Self::generate_from_rng(&mut rng,)
    }
}

impl SecretKey {
    #[must_use]
    pub fn new(key_pair: &KeyPair,) -> Self {
        let entropy = Zeroizing::new([0u8; ENTROPY_LEN],);
        Self {
            kp: key_pair.0.clone(),
            entropy,
        }
    }
}

impl PublicKey {
    #[must_use]
    pub fn new(key_pair: &KeyPair,) -> Self {
        key_pair.public()
    }
}

impl PublicKey {
    pub fn verify_sig(&self, msg: SignedMessage) -> bool {
       let pubk = self.0.clone();
        let sig = msg.sig().0;
        let binding = msg.serialize();
        let bytes = binding.as_slice();
        pubk.verify(bytes, &sig)
    }
}

impl SecretKey {
    /// Deterministic constructor from fixed entropy.
    fn generate_from_entropy(entropy: Entropy,) -> Self {
        // Hash the entropy → 32-byte seed, then into Keypair
        let binding = SigHasher::digest(entropy,);
        let kp: ed25519::Keypair =
            libp2p_identity::Keypair::ed25519_from_bytes(binding,)
                .expect("error",)
                .try_into()
                .unwrap();
        Self {
            kp,
            entropy: Zeroizing::new(entropy,),
        }
    }

    /// PRNG constructor.
    fn generate_from_rng<R: RngCore + CryptoRng,>(rng: &mut R,) -> Self {
        let mut entropy = [0u8; ENTROPY_LEN];
        rng.fill_bytes(entropy.as_mut(),);
        Self::generate_from_entropy(entropy,)
    }

    pub fn from_phrase(phrase: &str,) -> Result<Self,> {
        let m = Mnemonic::from_phrase(phrase, Language::English,).unwrap();
        let bytes = m.entropy();
        if bytes.len() != ENTROPY_LEN {
            bail!("mnemonic entropy must be {ENTROPY_LEN} bytes");
        }
        let mut e = [0u8; ENTROPY_LEN];
        e.copy_from_slice(bytes,);
        Ok(Self::generate_from_entropy(e,),)
    }

    // ---------- helpers -------------------------------------------

    #[must_use]
    pub fn phrase(&self,) -> String {
        let entropy = self.entropy.clone();
        let entropy = entropy.to_vec();
        let phrase = Mnemonic::from_entropy(entropy.as_slice(), Language::English,).unwrap();
        phrase.phrase().to_string()
    }
    #[must_use]
    pub fn secret_bytes(&self,) -> [u8; SECRET_LEN] {
        // copy because `SecretKey` keeps bytes private
        self.kp.secret().as_ref().try_into().unwrap()
    }

    pub fn sign<T: Serialize,>(&self, msg: &T,) -> Signature {
        let mut h = SigHasher::new();
        bincode::serialize_into(&mut h, msg,).unwrap();
        Signature(self.kp.sign(&h.finalize(),),)
    }
}

// ---------- serde impls --------------------------------------------

impl Serialize for SecretKey {
    fn serialize<S,>(&self, s: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        let mut bytes = self.secret_bytes().clone();
        s.serialize_bytes(bytes.as_mut(),)
    }
}
impl Serialize for PublicKey {
    fn serialize<S,>(&self, s: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        let mut bytes = self.to_bytes().clone();
        s.serialize_bytes(bytes.as_mut(),)
    }
}

impl<'de,> Deserialize<'de,> for SecretKey {
    fn deserialize<D,>(d: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let mut bytes: Vec<u8,> = <&[u8]>::deserialize(d,)?.to_vec();

        let kp = ed25519::Keypair::try_from_bytes(&mut bytes,)
            .map_err(|e| serde::de::Error::custom(e.to_string(),),)?;
        let entropy = Zeroizing::new([0u8; ENTROPY_LEN],); // unknown
        Ok(Self { kp, entropy, },)
    }
}

impl<'de,> Deserialize<'de,> for PublicKey {
    fn deserialize<D,>(d: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes: Vec<u8,> = <&[u8]>::deserialize(d,)?.to_vec();
        let kp = ed25519::PublicKey::try_from_bytes(&bytes,).expect("error",);

        Ok(Self(kp,),)
    }
}

// ---------- Debug --------------------------------------------------

impl Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "SigningKey({})", self.phrase())
    }
}

// --------------------------------------------------------------------
// VerifyingKey + Signature

#[derive(Clone, PartialOrd, PartialEq, Debug)]
pub struct PublicKey(pub ed25519::PublicKey,);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Signature(Vec<u8,>,);

#[derive(Clone, Debug,)]
pub struct KeyPair(pub ed25519::Keypair,);

impl KeyPair {
    #[must_use]
    pub fn generate() -> Self {
        let kp: ed25519::Keypair = ed25519::Keypair::generate();
        Self(kp,)
    }
}

impl KeyPair {
    #[must_use]
    pub fn from_bytes(bytes: &[u8],) -> Self {
        let mut bytes: [u8; 64] = bytes.try_into().unwrap();
        let kp = ed25519::Keypair::try_from_bytes(bytes.as_mut(),).expect("error",);
        Self::new(kp,)
    }

    #[must_use]
    pub const fn new(kp: ed25519::Keypair,) -> Self {
        Self(kp,)
    }

    #[must_use]
    pub fn peer_id(&self,) -> PeerId {
        self.public().to_peer_id()
    }

    #[must_use]
    pub fn secret(&self,) -> SecretKey {
        SecretKey::new(self,)
    }

    #[must_use]
    pub fn public(&self,) -> PublicKey {
        let pub_k = self.0.public();
        PublicKey(pub_k,)
    }
}

impl PublicKey {
    #[must_use]
    pub fn to_peer_id(&self,) -> PeerId {
        let pubk: libp2p_identity::PublicKey = self.0.clone().into();
        PeerId(pubk.to_peer_id(),)
    }

    #[must_use]
    pub fn to_bytes(&self,) -> [u8; 32] {
        self.0.to_bytes()
    }
}
impl Serialize for Signature {
    fn serialize<S,>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error,>
    where
        S: Serializer,
    {
        Vec::serialize(&self.0, serializer,)
    }
}

impl<'de,> Deserialize<'de,> for Signature {
    fn deserialize<D,>(d: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        Vec::deserialize(d,).map(Self,)
    }
}

// --------------------------------------------------------------------
// PeerId

#[derive(Clone, Copy, Eq, Hash,)]
pub struct PeerId(libp2p_identity::PeerId,);

impl Default for PeerId {
    fn default() -> Self {
        let id = libp2p_identity::PeerId::random();
        Self(id,)
    }
}

impl Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        let bytes = self.0.to_bytes();
        write!(f, "{}", bs58::encode(bytes).into_string())
    }
}

impl PartialEq<Self,> for PeerId {
    fn eq(&self, other: &Self,) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for PeerId {
    fn partial_cmp(&self, other: &Self,) -> Option<Ordering,> {
        self.0.partial_cmp(&other.0,)
    }
}

impl Ord for PeerId {
    fn cmp(&self, other: &Self,) -> Ordering {
        self.0.cmp(&other.0,)
    }
}
impl Serialize for PeerId {
    fn serialize<S,>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error,>
    where
        S: Serializer,
    {
        self.0.to_bytes().serialize(serializer,)
    }
}

impl<'de,> Deserialize<'de,> for PeerId {
    fn deserialize<D,>(d: D,) -> std::result::Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes: &[u8] = <&[u8]>::deserialize(d,)?;
        let peer_id =
            libp2p_identity::PeerId::from_bytes(bytes,).expect("error",);
        Ok(Self(peer_id,),)
    }
}

impl PeerId {
    #[must_use]
    pub fn digits(&self,) -> Vec<u8,> {
        self.0.to_bytes()
    }
}

impl Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "PeerId({self})")
    }
}

// -- keypair
impl Serialize for KeyPair {
    fn serialize<S,>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error,>
    where
        S: Serializer,
    {
        let keypair = self.0.clone();
        keypair.to_bytes().serialize(serializer,)
    }
}

impl<'de,> Deserialize<'de,> for KeyPair {
    fn deserialize<D,>(d: D,) -> std::result::Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes = <&[u8]>::deserialize(d,)?;
        let keypair = Self::from_bytes(bytes,);
        Ok(keypair,)
    }
}

// --------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::NetworkMessage;

    #[test]
    fn phrase_round_trip() {
        let sk = SecretKey::default();
        let sk2 = SecretKey::from_phrase(&sk.phrase(),).unwrap();
        assert_eq!(sk.secret_bytes(), sk2.secret_bytes());
    }

    #[test]
    fn sign_and_verify() {
        let kp = KeyPair::generate();
        let pubk = kp.public();
        let msg = SignedMessage::new(&kp, NetworkMessage::Dummy);
        assert!(pubk.verify_sig(msg));
    }
}
