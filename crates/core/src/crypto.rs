//! crates/core/src/crypto.rs
//! A *working* rewrite that uses `libp2p-identity` instead of `ed25519-dalek`.

use std::convert::TryInto;
use std::fmt;
use std::ops::Deref;

use anyhow::{Result, bail};
use bip39::Mnemonic;
use blake2::digest::consts;
use blake2::{Blake2s, Digest};
use libp2p_identity::ed25519;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
pub struct SigningKey {
    kp:      ed25519::Keypair, // libp2p (public+secret)
    entropy: Zeroizing<Entropy,>,
}

// ---------- constructors -------------------------------------------

impl Default for SigningKey {
    fn default() -> Self {
        let mut rng = StdRng::from_os_rng();
        Self::generate_from_rng(&mut rng,)
    }
}

impl SigningKey {
    pub fn new(key_pair: &KeyPair,) -> Self {
        let entropy = Zeroizing::new([0u8; ENTROPY_LEN],);
        Self {
            kp: key_pair.0.clone(),
            entropy,
        }
    }
}

impl SigningKey {
    /// Deterministic constructor from fixed entropy.
    fn generate_from_entropy(entropy: Entropy,) -> Self {
        // Hash the entropy → 32-byte seed, then into Keypair
        let binding = SigHasher::digest(entropy,).clone();
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
        rng.fill_bytes(&mut entropy,);
        Self::generate_from_entropy(entropy,)
    }

    pub fn from_phrase(phrase: &str,) -> Result<Self,> {
        let m = Mnemonic::from_phrase(phrase, Default::default(),)?;
        let bytes = m.entropy();
        if bytes.len() != ENTROPY_LEN {
            bail!("mnemonic entropy must be {ENTROPY_LEN} bytes");
        }
        let mut e = [0u8; ENTROPY_LEN];
        e.copy_from_slice(bytes,);
        Ok(Self::generate_from_entropy(e,),)
    }

    // ---------- helpers -------------------------------------------

    pub fn phrase(&self,) -> String {
        Mnemonic::from_entropy(&*self.entropy, Default::default(),)
            .unwrap()
            .phrase()
            .to_owned()
    }

    pub fn peer_id(&self,) -> PeerId {
        VerifyingKey(self.kp.public(),).to_peer_id()
    }

    pub fn verifying_key(&self,) -> VerifyingKey {
        VerifyingKey(self.kp.public(),)
    }

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

impl Serialize for SigningKey {
    fn serialize<S,>(&self, s: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        s.serialize_bytes(&self.secret_bytes(),)
    }
}
impl Serialize for VerifyingKey {
    fn serialize<S,>(&self, s: S,) -> Result<S::Ok, S::Error,>
    where S: Serializer {
        s.serialize_bytes(&self.to_bytes(),)
    }
}

impl<'de,> Deserialize<'de,> for SigningKey {
    fn deserialize<D,>(d: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes: Vec<u8,> = <&[u8]>::deserialize(d,)?.to_vec();

        let kp = ed25519::Keypair::try_from_bytes(&mut bytes.clone(),)
            .map_err(|e| serde::de::Error::custom(e.to_string(),),)?;
        let entropy = Zeroizing::new([0u8; ENTROPY_LEN],); // unknown
        Ok(SigningKey { kp, entropy, },)
    }
}

impl<'de,> Deserialize<'de,> for VerifyingKey {
    fn deserialize<D,>(d: D,) -> Result<Self, D::Error,>
    where D: Deserializer<'de,> {
        let bytes: Vec<u8,> = <&[u8]>::deserialize(d,)?.to_vec();
        let kp =
            ed25519::PublicKey::try_from_bytes(bytes.deref(),).expect("error",);

        Ok(VerifyingKey(kp,),)
    }
}

// ---------- Debug --------------------------------------------------

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "SigningKey({})", self.phrase())
    }
}

// --------------------------------------------------------------------
// VerifyingKey + Signature

#[derive(Clone,)]
pub struct VerifyingKey(pub ed25519::PublicKey,);

#[derive(Clone,)]
pub struct Signature(Vec<u8,>,);

#[derive(Clone, Debug,)]
pub struct KeyPair(pub ed25519::Keypair,);

impl KeyPair {
    pub fn default() -> Self {
        let kp: ed25519::Keypair = ed25519::Keypair::generate();
        KeyPair(kp,)
    }
    pub fn from_bytes(bytes: &[u8],) -> Self {
        let mut bytes: [u8; 64] = bytes.try_into().unwrap();
        let kp = ed25519::Keypair::try_from_bytes(&mut bytes,).expect("error",);
        KeyPair::new(kp,)
    }

    pub fn new(kp: ed25519::Keypair,) -> Self {
        KeyPair(kp,)
    }

    pub fn to_peer_id(self,) -> PeerId {
        let pubk: libp2p_identity::PublicKey = self.0.public().into();
        PeerId(pubk.to_peer_id(),)
    }
}

impl VerifyingKey {
    pub fn verify<T: Serialize,>(&self, msg: &T, sig: &Signature,) -> bool {
        let mut h = SigHasher::new();
        bincode::serialize_into(&mut h, msg,).unwrap();
        self.0.verify(&h.finalize(), &sig.0.as_ref(),)
    }

    pub fn to_peer_id(&self,) -> PeerId {
        let pubk: libp2p_identity::PublicKey = self.0.clone().into();
        PeerId(pubk.to_peer_id(),)
    }

    pub fn to_bytes(&self,) -> [u8; 32] {
        self.0.to_bytes()
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(
            f,
            "VerifyingKey({})",
            bs58::encode(self.0.to_bytes()).into_string()
        )
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(
            f,
            "Signature({})",
            bs58::encode::<&Vec<u8,>,>(self.0.as_ref()).into_string()
        )
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
        Vec::deserialize(d,).map(|v| Self(v,),)
    }
}

// --------------------------------------------------------------------
// PeerId

#[derive(Clone, Copy, PartialEq, Eq, Hash,)]
pub struct PeerId(libp2p_identity::PeerId,);

impl Default for PeerId {
    fn default() -> Self {
        let id = libp2p_identity::PeerId::random();
        PeerId(id)
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
        Ok(PeerId(peer_id,),)
    }
}

impl PeerId {
    pub fn digits(&self,) -> Vec<u8,> {
        self.0.to_bytes()
    }
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "PeerId({})", self)
    }
}
impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        self.0.to_string().fmt(f,)
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
        let keypair = KeyPair::from_bytes(bytes,);
        Ok(keypair,)
    }
}

// --------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_round_trip() {
        let sk = SigningKey::default();
        let sk2 = SigningKey::from_phrase(&sk.phrase(),).unwrap();
        assert_eq!(sk.secret_bytes(), sk2.secret_bytes());
    }

    #[test]
    fn sign_and_verify() {
        #[derive(Serialize,)]
        struct Msg {
            a: u32,
        }
        let sk = SigningKey::default();
        let sig = sk.sign(&Msg { a: 123, },);
        assert!(sk.verifying_key().verify(&Msg { a: 123, }, &sig));
    }
}
