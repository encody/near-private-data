use anyhow::Result;
use bellperson::groth16::{prepare_verifying_key, VerifyingKey};
pub use bellperson::{
    gadgets::{
        boolean::{AllocatedBit, Boolean},
        multipack,
        sha256::sha256,
    },
    groth16::{self, Parameters, PreparedVerifyingKey, Proof},
    Circuit, ConstraintSystem, SynthesisError,
};
pub use blstrs::Bls12;
use ff::PrimeField;
use pairing::Engine;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use std::{fs::File, path::PathBuf, ptr::read};

// TODO: this needs to migrate to something non-interactive like spartan for setup and groth is a little inefficient.
// We also don't need contracts to verify these, although we can

// TODO: if we stick with this, we need to use something quantum secure at some point
/// Our own SHA-256d gadget. Input and output are in little-endian bit order.
fn sha256d<Scalar: PrimeField, CS: ConstraintSystem<Scalar>>(
    mut cs: CS,
    data: &[Boolean],
) -> Result<Vec<Boolean>, SynthesisError> {
    // Flip endianness of each input byte
    let input: Vec<_> = data
        .chunks(8)
        .map(|c| c.iter().rev())
        .flatten()
        .cloned()
        .collect();

    let mid = sha256(cs.namespace(|| "SHA-256(input)"), &input)?;
    let res = sha256(cs.namespace(|| "SHA-256(mid)"), &mid)?;

    // Flip endianness of each output byte
    Ok(res
        .chunks(8)
        .map(|c| c.iter().rev())
        .flatten()
        .cloned()
        .collect())
}

struct VerifiableHash<const PREIMAGE: usize> {
    /// The input to SHA-256d we are proving that we know. Set to `None` when we
    /// are verifying a proof (and do not have the witness data).
    preimage: Option<[u8; PREIMAGE]>,
}

impl<Scalar: PrimeField, const PREIMAGE_SIZE: usize> Circuit<Scalar>
    for VerifiableHash<PREIMAGE_SIZE>
{
    fn synthesize<CS: ConstraintSystem<Scalar>>(self, cs: &mut CS) -> Result<(), SynthesisError> {
        // Compute the values for the bits of the preimage. If we are verifying a proof,
        // we still need to create the same constraints, so we return an equivalent-size
        // Vec of None (indicating that the value of each bit is unknown).
        let bit_values = if let Some(preimage) = self.preimage {
            preimage
                .iter()
                .map(|byte| (0..8).map(move |i| (byte >> i) & 1u8 == 1u8))
                .flatten()
                .map(|b| Some(b))
                .collect()
        } else {
            vec![None; PREIMAGE_SIZE * 8]
        };
        assert_eq!(bit_values.len(), PREIMAGE_SIZE * 8);

        // Witness the bits of the preimage.
        let preimage_bits = bit_values
            .into_iter()
            .enumerate()
            // Allocate each bit.
            .map(|(i, b)| AllocatedBit::alloc(cs.namespace(|| format!("preimage bit {}", i)), b))
            // Convert the AllocatedBits into Booleans (required for the sha256 gadget).
            .map(|b| b.map(Boolean::from))
            .collect::<Result<Vec<_>, _>>()?;

        // Compute hash = SHA-256d(preimage).
        let hash = sha256d(cs.namespace(|| "SHA-256d(preimage)"), &preimage_bits)?;

        // Expose the vector of 32 boolean variables as compact public inputs.
        multipack::pack_into_inputs(cs.namespace(|| "pack hash"), &hash)
    }
}

fn insecure_parameters<const PREIMAGE_SIZE: usize>() -> groth16::Parameters<Bls12> {
    let c = VerifiableHash::<PREIMAGE_SIZE> { preimage: None };
    groth16::generate_random_parameters::<Bls12, _, _>(c, &mut OsRng).unwrap()
}

pub struct ShaPreimageProver<const PREIMAGE_SIZE: usize> {
    preimage: [u8; PREIMAGE_SIZE],
    params: Parameters<Bls12>,
}

impl<const PREIMAGE_SIZE: usize> ShaPreimageProver<PREIMAGE_SIZE> {
    pub fn new(preimage: [u8; PREIMAGE_SIZE], params: Option<Parameters<Bls12>>) -> Self {
        Self {
            preimage,
            params: params.unwrap_or(insecure_parameters::<PREIMAGE_SIZE>()),
        }
    }

    pub fn prove(&self) -> groth16::Proof<Bls12> {
        // Create an instance of our circuit (with the preimage as a witness).
        let c = VerifiableHash {
            preimage: Some(self.preimage),
        };

        // Create a Groth16 proof with our parameters.
        let proof = groth16::create_random_proof(c, &self.params, &mut OsRng).unwrap();

        proof
    }

    fn verify(self, proof: &Proof<Bls12>, hash: &[u8], pvk: &PreparedVerifyingKey<Bls12>) -> bool {
        crate::verify(proof, hash, pvk)
    }
}

pub fn verify(proof: &Proof<Bls12>, hash: &[u8], pvk: &PreparedVerifyingKey<Bls12>) -> bool {
    // Pack the hash as inputs for proof verification.
    let hash_bits = multipack::bytes_to_bits_le(&hash);

    let inputs = multipack::compute_multipacking::<<Bls12 as Engine>::Fr>(&hash_bits);

    // Check the proof!
    groth16::verify_proof(pvk, proof, &inputs).unwrap()
}

pub fn read_pvk(path: &PathBuf) -> Result<PreparedVerifyingKey<Bls12>> {
    Ok(prepare_verifying_key(&read_vk(path)?))
}

pub fn read_vk(path: &PathBuf) -> Result<VerifyingKey<Bls12>> {
    let file = File::open(path)?;
    let vk = VerifyingKey::<Bls12>::read(file)?;
    Ok(vk)
}

pub fn write_vk(path: &PathBuf, vk: &VerifyingKey<Bls12>) -> Result<()> {
    let file = File::create(path)?;
    file.set_len(0)?; // Clear the file

    vk.write(file)?;
    Ok(())
}
pub fn read_params(path: &PathBuf, should_check: bool) -> Result<Parameters<Bls12>> {
    let file = File::open(path)?;
    let vk = Parameters::<Bls12>::read(file, should_check)?;
    Ok(vk)
}

pub fn write_params(path: &PathBuf, params: &Parameters<Bls12>) -> Result<()> {
    let file = File::create(path)?;
    file.set_len(0)?; // Clear the file

    params.write(file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn generate_dumb_vk_file() {
        let path = PathBuf::from_str("../pvk.key").unwrap();
        let params = insecure_parameters::<32>();
        write_vk(&path, &params.vk).unwrap();

        read_pvk(&path).expect("Generated a bad pvk");
    }

    #[test]
    fn generate_dumb_params() {
        let path = PathBuf::from_str("../params.key").unwrap();
        let params = insecure_parameters::<32>();
        write_params(&path, &params).unwrap();
        read_params(&path, true).expect("Generated a bad parameter file");
        
        let path = PathBuf::from_str("../pvk.key").unwrap();
        write_vk(&path, &params.vk).unwrap();
        read_pvk(&path).expect("Generated a bad pvk");

    }

    #[test]
    fn test_verify() {
        let preimage = [50_u8; 32];
        // Create parameters for our circuit. In a production deployment these would
        // be generated securely using a multiparty computation.
        let params = insecure_parameters::<32>();
        // Prepare the verification key (for proof verification).
        let pvk = groth16::prepare_verifying_key(&params.vk);

        // Compute the hash of the preimage
        let hash = Sha256::digest(&Sha256::digest(&preimage));

        let prover = ShaPreimageProver::<32>::new(preimage, Some(params));

        let proof = prover.prove();

        assert!(verify(&proof, &hash, &pvk))
    }
}
