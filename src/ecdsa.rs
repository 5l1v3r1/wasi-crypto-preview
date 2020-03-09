use parking_lot::Mutex;
use std::sync::Arc;
use zeroize::Zeroize;

use super::error::*;
use super::handles::*;
use super::signature::*;
use super::signature_keypair::*;
use super::WASI_CRYPTO_CTX;

#[derive(Clone, Copy, Debug)]
pub struct ECDSASignatureOp {
    pub alg: SignatureAlgorithm,
}

impl ECDSASignatureOp {
    pub fn new(alg: SignatureAlgorithm) -> Self {
        ECDSASignatureOp { alg }
    }
}

#[derive(Debug, Clone)]
pub struct ECDSASignatureKeyPair {
    pub alg: SignatureAlgorithm,
    pub pkcs8: Vec<u8>,
    pub ring_kp: Arc<ring::signature::EcdsaKeyPair>,
}

impl Drop for ECDSASignatureKeyPair {
    fn drop(&mut self) {
        self.pkcs8.zeroize();
    }
}

impl ECDSASignatureKeyPair {
    fn ring_alg_from_alg(
        alg: SignatureAlgorithm,
    ) -> Result<&'static ring::signature::EcdsaSigningAlgorithm, Error> {
        let ring_alg = match alg {
            SignatureAlgorithm::ECDSA_P256_SHA256 => {
                &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING
            }
            SignatureAlgorithm::ECDSA_P384_SHA384 => {
                &ring::signature::ECDSA_P384_SHA384_FIXED_SIGNING
            }
            _ => bail!("Unsupported signature system"),
        };
        Ok(ring_alg)
    }

    pub fn from_pkcs8(alg: SignatureAlgorithm, pkcs8: &[u8]) -> Result<Self, Error> {
        let ring_alg = Self::ring_alg_from_alg(alg)?;
        let ring_kp = ring::signature::EcdsaKeyPair::from_pkcs8(ring_alg, pkcs8)
            .map_err(|_| anyhow!("Invalid key pair"))?;
        let kp = ECDSASignatureKeyPair {
            alg,
            pkcs8: pkcs8.to_vec(),
            ring_kp: Arc::new(ring_kp),
        };
        Ok(kp)
    }

    pub fn as_pkcs8(&self) -> Result<&[u8], Error> {
        Ok(&self.pkcs8)
    }

    pub fn generate(alg: SignatureAlgorithm) -> Result<Self, Error> {
        let ring_alg = Self::ring_alg_from_alg(alg)?;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(ring_alg, &rng)
            .map_err(|_| anyhow!("RNG error"))?;
        Self::from_pkcs8(alg, pkcs8.as_ref())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ECDSASignatureKeyPairBuilder {
    pub alg: SignatureAlgorithm,
}

impl ECDSASignatureKeyPairBuilder {
    pub fn new(alg: SignatureAlgorithm) -> Self {
        ECDSASignatureKeyPairBuilder { alg }
    }

    pub fn generate(&self) -> Result<Handle, Error> {
        let kp = ECDSASignatureKeyPair::generate(self.alg)?;
        let handle = WASI_CRYPTO_CTX
            .signature_keypair_manager
            .register(SignatureKeyPair::ECDSA(kp))?;
        Ok(handle)
    }

    pub fn import(&self, encoded: &[u8], encoding: KeyPairEncoding) -> Result<Handle, Error> {
        match encoding {
            KeyPairEncoding::PKCS8 => {}
            _ => bail!("Unsupported"),
        };
        let kp = ECDSASignatureKeyPair::from_pkcs8(self.alg, encoded)?;
        let handle = WASI_CRYPTO_CTX
            .signature_keypair_manager
            .register(SignatureKeyPair::ECDSA(kp))?;
        Ok(handle)
    }
}

#[derive(Debug)]
pub struct ECDSASignatureState {
    pub kp: ECDSASignatureKeyPair,
    pub input: Mutex<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ECDSASignature(pub Vec<u8>);

impl AsRef<[u8]> for ECDSASignature {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl ECDSASignatureState {
    pub fn new(kp: ECDSASignatureKeyPair) -> Self {
        ECDSASignatureState {
            kp,
            input: Mutex::new(vec![]),
        }
    }

    pub fn update(&self, input: &[u8]) -> Result<(), Error> {
        self.input.lock().extend_from_slice(input);
        Ok(())
    }

    pub fn sign(&self) -> Result<ECDSASignature, Error> {
        let rng = ring::rand::SystemRandom::new();
        let input = self.input.lock();
        let signature_u8 = self
            .kp
            .ring_kp
            .sign(&rng, &input)
            .map_err(|_| anyhow!("Unable to sign"))?
            .as_ref()
            .to_vec();
        let signature = ECDSASignature(signature_u8);
        Ok(signature)
    }
}
