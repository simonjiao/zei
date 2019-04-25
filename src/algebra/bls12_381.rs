use super::groups::{Group, Scalar};
use super::pairing::Pairing;
use rand::{CryptoRng, Rng};
use rand_04::Rand;
use digest::Digest;
use digest::generic_array::typenum::U64;
use crate::utils::u8_bigendian_slice_to_u32;
use std::fmt;
use pairing::bls12_381::{Fr, G1, G2, Fq12, FrRepr};
use pairing::{PrimeField, Field, EncodedPoint};
use pairing::{CurveProjective,CurveAffine};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BLSScalar(pub(crate) Fr);
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BLSG1(pub(crate) G1);
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BLSG2(pub(crate) G2);
#[derive(Clone, PartialEq, Eq)]
pub struct BLSGt(pub(crate) Fq12);

impl Scalar for BLSScalar {
    // scalar generation
    fn random_scalar<R: CryptoRng + Rng>(rng: &mut R) -> BLSScalar{
        // hack to use rand_04::Rng rather than rand::Rng
        let mut random_bytes = [0u8;16];
        rng.fill_bytes(&mut random_bytes);
        let mut seed = [0u32;4];
        for i in 0..4{
            seed[i] = u8_bigendian_slice_to_u32(&random_bytes[i*4..(i+1)*4]);
        }

        use rand_04::SeedableRng;
        let mut prng_04 = rand_04::ChaChaRng::from_seed(&seed);
        BLSScalar(Fr::rand(&mut prng_04))
    }

    fn from_u32(value: u32) -> BLSScalar{
        Self::from_u64(value as u64)
    }

    fn from_u64(value: u64) -> BLSScalar {
        let mut v  = value;
        let mut result = Fr::zero();
        let mut two_pow_i = Fr::one();
        for _ in 0..64{
            if v == 0 {break;}
            if v&1 == 1u64 {
                result.add_assign(&two_pow_i);
                //result = result + two_pow_i;
            }
            v = v>>1;
            two_pow_i.double();// = two_pow_i * two;
        }
        BLSScalar(result)
    }

    fn from_hash<D>(hash: D) -> BLSScalar
        where D: Digest<OutputSize = U64> + Default{
        let result = hash.result();
        let mut seed = [0u32; 16];
        for i in 0..16{
            seed[i] = u8_bigendian_slice_to_u32(&result.as_slice()[i*4..(i+1)*4]);
        }
        use rand_04::SeedableRng;
        let mut prng = rand_04::ChaChaRng::from_seed(&seed);
        BLSScalar(Fr::rand(&mut prng))
    }

    // scalar arithmetic
    fn add(&self, b: &BLSScalar) -> BLSScalar{
        let mut m = self.0.clone();
        m.add_assign(&b.0);
        BLSScalar(m)
    }
    fn mul(&self, b: &BLSScalar) -> BLSScalar{
        let mut m = self.0.clone();
        m.mul_assign(&b.0);
        BLSScalar(m)
    }

    //scalar serialization
    fn to_bytes(&self) -> Vec<u8>{
        let repr = FrRepr::from(self.0);
        let mut v = vec![];
        for a in &repr.0 {
            let array = crate::utils::u64_to_bigendian_u8array(*a);
            v.extend_from_slice(&array[..])
        }
        v
    }

    fn from_bytes(bytes: &[u8]) -> BLSScalar {
        let mut repr_array = [0u64; 4];
        for i in 0..4 {
            let slice = &bytes[i * 8..i * 8 + 8];
            repr_array[i]  = crate::utils::u8_bigendian_slice_to_u64(slice);

        }
        let fr_repr = FrRepr(repr_array);
        BLSScalar(Fr::from_repr(fr_repr).unwrap())
    }
}

impl Group for BLSG1{
    type ScalarType = BLSScalar;
    const COMPRESSED_LEN: usize = 48;
    const SCALAR_BYTES_LEN: usize = 32;
    fn get_identity() -> BLSG1{
        BLSG1(G1::zero())
    }
    fn get_base() -> BLSG1{
        BLSG1(G1::one())
    }

    // compression/serialization helpers
    fn to_compressed_bytes(&self) -> Vec<u8>{
        let v = self.0.into_affine().into_compressed().as_ref().to_vec();
        v
    }
    fn from_compressed_bytes(bytes: &[u8]) -> Option<BLSG1>{
        let some: G1 = G1::one();
        let mut compressed = some.into_affine().into_compressed();
        let mut_bytes = compressed.as_mut();
        for i in 0..48{
            mut_bytes[i] = bytes[i];
        }
        let affine = compressed.into_affine().unwrap();
        let g1 = G1::from(affine);

        Some(BLSG1(g1))
    }

    //arithmetic
    fn mul(&self, scalar: &BLSScalar) -> BLSG1 {
        let mut m = self.0.clone();
        m.mul_assign(scalar.0);
        BLSG1(m)
    }
    fn add(&self, other: &Self) -> BLSG1{
        let mut m = self.0.clone();
        m.add_assign(&other.0);
        BLSG1(m)
    }
    fn sub(&self, other: &Self) -> BLSG1{
        let mut m = self.0.clone();
        m.sub_assign(&other.0);
        BLSG1(m)
    }
}

impl Group for BLSG2{
    type ScalarType = BLSScalar;
    const COMPRESSED_LEN: usize = 96; // TODO
    const SCALAR_BYTES_LEN: usize = 32; // TODO
    fn get_identity() -> BLSG2{
        BLSG2(G2::zero())
    }
    fn get_base() -> BLSG2{
        BLSG2(G2::one())
    }

    // compression/serialization helpers
    fn to_compressed_bytes(&self) -> Vec<u8>{
        let v = self.0.into_affine().into_compressed().as_ref().to_vec();
        v
    }
    fn from_compressed_bytes(bytes: &[u8]) -> Option<BLSG2>{
        let some: G2 = G2::one();
        let mut compressed = some.into_affine().into_compressed();
        let mut_bytes = compressed.as_mut();
        for i in 0..96{
            mut_bytes[i] = bytes[i];
        }
        let affine = compressed.into_affine().unwrap();
        let g2 = G2::from(affine);

        Some(BLSG2(g2))
    }

    //arithmetic
    fn mul(&self, scalar: &BLSScalar) -> BLSG2 {
        let mut m = self.0.clone();
        m.mul_assign(scalar.0);
        BLSG2(m)
        //return BLSG2(self.0 * scalar.0)
    }
    fn add(&self, other: &Self) -> BLSG2{
        let mut m = self.0.clone();
        m.add_assign(&other.0);
        BLSG2(m)
    }
    fn sub(&self, other: &Self) -> BLSG2{
        let mut m = self.0.clone();
        m.sub_assign(&other.0);
        BLSG2(m)
    }
}

impl fmt::Debug for BLSGt{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Fr: Some Gt Element")
    }
}

impl Pairing for BLSGt {
    type G1 = BLSG1;
    type G2 = BLSG2;
    type ScalarType = BLSScalar;

    fn pairing(a: &Self::G1, b: &Self::G2) -> Self{
        BLSGt( a.0.into_affine().pairing_with(&b.0.into_affine()))
    }
    fn scalar_mul(&self, a: &Self::ScalarType) -> BLSGt{

        let r = self.0.pow(a.0.into_repr().as_ref());
        BLSGt(r)
    }
    fn add(&self, other: &Self) -> BLSGt{
        let mut m = other.0.clone();
        m.add_assign(&self.0);
        BLSGt(m)
    }

    fn g1_mul_scalar(a: &Self::G1, b: &Self::ScalarType) -> Self::G1{
        a.mul(b)
    }
    fn g2_mul_scalar(a: &Self::G2, b: &Self::ScalarType) -> Self::G2{
        a.mul(b)
    }
}

#[cfg(test)]
mod bls12_381_groups_test{
    use crate::algebra::groups::group_tests::{test_scalar_operations, test_scalar_serializarion};

    #[test]
    fn test_scalar_ops(){
        test_scalar_operations::<super::BLSScalar>();
    }

    #[test]
    fn test_scalar_serialization(){
        test_scalar_serializarion::<super::BLSScalar>();
    }
}

#[cfg(test)]
mod elgamal_over_bls_groups {
    use crate::basic_crypto::elgamal::elgamal_test;

    #[test]
    fn verification_g1(){
        elgamal_test::verification::<super::BLSG1>();
    }

    #[test]
    fn decryption_g1(){
        elgamal_test::decryption::<super::BLSG1>();
    }

    #[test]
    fn to_json_g1(){
        elgamal_test::to_json::<super::BLSG1>();
    }


    #[test]
    fn to_message_pack_g1(){
        elgamal_test::to_message_pack::<super::BLSG1>();
    }

    #[test]
    fn verification_g2(){
        elgamal_test::verification::<super::BLSG1>();
    }

    #[test]
    fn decryption_g2(){
        elgamal_test::decryption::<super::BLSG2>();
    }

    #[test]
    fn to_json_g2(){
        elgamal_test::to_json::<super::BLSG2>();
    }

    #[test]
    fn to_message_pack_g2(){
        elgamal_test::to_message_pack::<super::BLSG2>();
    }

}

#[cfg(test)]
mod credentials_over_bls_12_381 {

    #[test]
    fn single_attribute(){
        crate::credentials::credentials_tests::single_attribute::<super::BLSGt>();
    }

    #[test]
    fn two_attributes(){
        crate::credentials::credentials_tests::two_attributes::<super::BLSGt>();
    }
}