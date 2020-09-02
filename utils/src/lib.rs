pub mod errors;
pub mod serialization;
#[macro_export]
macro_rules! serialize_deserialize {
  ($t:ident) => {
    impl serde::Serialize for $t {
      fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
      {
        if serializer.is_human_readable() {
          serializer.serialize_str(&utils::b64enc(&self.zei_to_bytes()))
        } else {
          serializer.serialize_bytes(&self.zei_to_bytes())
        }
      }
    }

    impl<'de> serde::Deserialize<'de> for $t {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: serde::Deserializer<'de>
      {
        let bytes = if deserializer.is_human_readable() {
          deserializer.deserialize_str(utils::serialization::zei_obj_serde::BytesVisitor)?
        } else {
          deserializer.deserialize_bytes(utils::serialization::zei_obj_serde::BytesVisitor)?
        };
        $t::zei_from_bytes(bytes.as_slice()).map_err(serde::de::Error::custom)
      }
    }
  };
}

/// I convert a 8 byte array big-endian into a u64 (bigendian)
pub fn u8_be_slice_to_u64(slice: &[u8]) -> u64 {
  let mut a = [0u8; 8];
  a.copy_from_slice(slice);
  u64::from_be_bytes(a)
}

/// I convert a 8 byte array little-endian into a u64 (bigendian)
pub fn u8_le_slice_to_u64(slice: &[u8]) -> u64 {
  let mut a = [0u8; 8];
  a.copy_from_slice(slice);
  u64::from_le_bytes(a)
}

/// I convert a slice into a u32 (bigendian)
pub fn u8_be_slice_to_u32(slice: &[u8]) -> u32 {
  let mut a = [0u8; 4];
  a.copy_from_slice(slice);
  u32::from_be_bytes(a)
}

/// I convert a slice into a u32 (littleendian)
pub fn u8_le_slice_to_u32(slice: &[u8]) -> u32 {
  let mut a = [0u8; 4];
  a.copy_from_slice(slice);
  u32::from_le_bytes(a)
}

/// I compute the minimum power of two that is greater or equal to the input
pub fn min_greater_equal_power_of_two(n: u32) -> u32 {
  2.0f64.powi((n as f64).log2().ceil() as i32) as u32
}

pub fn u64_to_u32_pair(x: u64) -> (u32, u32) {
  ((x & 0xFFFF_FFFF) as u32, (x >> 32) as u32)
}

pub fn b64enc<T: ?Sized + AsRef<[u8]>>(input: &T) -> String {
  base64::encode_config(input, base64::URL_SAFE)
}
pub fn b64dec<T: ?Sized + AsRef<[u8]>>(input: &T) -> Result<Vec<u8>, base64::DecodeError> {
  base64::decode_config(input, base64::URL_SAFE)
}

#[cfg(test)]
mod test {

  #[test]
  fn test_u8_be_slice_to_u32() {
    let array = [0xFA as u8, 0x01 as u8, 0xC6 as u8, 0x73 as u8];
    let n = super::u8_be_slice_to_u32(&array);
    assert_eq!(0xFA01C673, n);
  }

  #[test]
  fn u8_be_slice_to_u64() {
    let array = [0xFA as u8, 0x01 as u8, 0xC6 as u8, 0x73 as u8, 0x22, 0xE4, 0x98, 0xA2];
    let n = super::u8_be_slice_to_u64(&array);
    assert_eq!(0xFA01C67322E498A2, n);
  }

  #[test]
  fn min_greater_equal_power_of_two() {
    assert_eq!(16, super::min_greater_equal_power_of_two(16));
    assert_eq!(16, super::min_greater_equal_power_of_two(15));
    assert_eq!(16, super::min_greater_equal_power_of_two(9));
    assert_eq!(8, super::min_greater_equal_power_of_two(8));
    assert_eq!(8, super::min_greater_equal_power_of_two(6));
    assert_eq!(8, super::min_greater_equal_power_of_two(5));
    assert_eq!(4, super::min_greater_equal_power_of_two(4));
    assert_eq!(4, super::min_greater_equal_power_of_two(3));
    assert_eq!(2, super::min_greater_equal_power_of_two(2));
    assert_eq!(1, super::min_greater_equal_power_of_two(1));
    assert_eq!(0, super::min_greater_equal_power_of_two(0));
  }

  #[test]
  fn u64_to_u32_pair() {
    assert_eq!((32, 0), super::u64_to_u32_pair(32u64));
    assert_eq!((0xFFFFFFFF, 0xFFFFFFFF),
               super::u64_to_u32_pair(0xFFFFFFFFFFFFFFFFu64));
    assert_eq!((0, 0xFFFFFFFF),
               super::u64_to_u32_pair(0xFFFFFFFF00000000u64));
  }
}