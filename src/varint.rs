//! Encoding of the integers of the SCPF heap: varints and field elements.
//!
//! Varints are big-endian with a prefix that encodes their length: the number of leading one bits
//! of the first byte is the number of bytes that follow it, so 1 byte holds 7 bits, 2 bytes hold 14
//! bits, and so on up to 8 bytes holding 56 bits. Values of 57 bits or more take 9 bytes: `0xff`
//! followed by the 64-bit value. Encoders always use the shortest encoding, but decoders accept
//! non-minimal ones.
//!
//! Field elements are inline words: the big-endian representation of their canonical integer form,
//! in exactly [`Field::LEN`] bytes.
//!
//! Encoders append to a byte vector. Decoders read from the beginning of a byte slice and return
//! the decoded value together with the number of bytes it takes up, so that the caller can advance
//! past it.

use anyhow::{Context, Result};
use starkom_ff::Field;

/// Appends the varint encoding of `value` to `bytes`.
pub fn encode_usize(value: usize, bytes: &mut Vec<u8>) {
    encode_u64(value as u64, bytes);
}

/// Decodes the varint at the beginning of `bytes`, returning its value and its length in bytes.
///
/// Returns an error if `bytes` is truncated or if the value doesn't fit in a `usize`, which can
/// only happen on targets with pointers of less than 64 bits.
pub fn decode_usize(bytes: &[u8]) -> Result<(usize, usize)> {
    let (value, length) = decode_u64(bytes)?;
    let value = usize::try_from(value).context("varint overflow")?;
    Ok((value, length))
}

/// Appends `value` to `bytes` as an inline word.
pub fn encode_field_element<F: Field>(value: F, bytes: &mut Vec<u8>) {
    let word = value.to_u512().to_big_endian();
    bytes.extend_from_slice(&word[(word.len() - F::LEN)..]);
}

/// Decodes the inline word at the beginning of `bytes` as a field element, returning it together
/// with the length of the word in bytes, which is always [`Field::LEN`].
///
/// Returns an error if `bytes` is truncated or if the word isn't a canonical field element.
pub fn decode_field_element<F: Field>(bytes: &[u8]) -> Result<(F, usize)> {
    let word = bytes.get(..F::LEN).context("truncated field element")?;
    let value = Option::from(F::try_from_be_bytes(word)).context("non-canonical field element")?;
    Ok((value, F::LEN))
}

fn encode_u64(value: u64, bytes: &mut Vec<u8>) {
    match (1..=8).find(|length| value >> (7 * length) == 0) {
        Some(length) => {
            let prefix = ((1u64 << (length - 1)) - 1) << (7 * length + 1);
            bytes.extend_from_slice(&(prefix | value).to_be_bytes()[(8 - length)..]);
        }
        None => {
            bytes.push(0xff);
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn decode_u64(bytes: &[u8]) -> Result<(u64, usize)> {
    let first = *bytes.first().context("truncated varint")?;
    let length = first.leading_ones() as usize + 1;
    let varint = bytes.get(..length).context("truncated varint")?;
    let mut value = u64::from(first) & (0x7f >> (length - 1));
    for &byte in &varint[1..] {
        value = (value << 8) | u64::from(byte);
    }
    Ok((value, length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use starkom_bluesky::Scalar;
    use starkom_koalabear::{KB, KB8};

    fn test_encode_usize_impl(value: usize, expected: &[u8]) {
        let mut bytes = vec![0x42];
        encode_usize(value, &mut bytes);
        assert_eq!(bytes, [&[0x42], expected].concat());
    }

    #[test]
    fn test_encode_usize() {
        test_encode_usize_impl(0, &[0x00]);
        test_encode_usize_impl(1, &[0x01]);
        test_encode_usize_impl(127, &[0x7f]);
        test_encode_usize_impl(128, &[0x80, 0x80]);
        test_encode_usize_impl(300, &[0x81, 0x2c]);
        test_encode_usize_impl(16383, &[0xbf, 0xff]);
        test_encode_usize_impl(16384, &[0xc0, 0x40, 0x00]);
        test_encode_usize_impl((1 << 21) - 1, &[0xdf, 0xff, 0xff]);
        test_encode_usize_impl(1 << 21, &[0xe0, 0x20, 0x00, 0x00]);
        test_encode_usize_impl((1 << 28) - 1, &[0xef, 0xff, 0xff, 0xff]);
        test_encode_usize_impl(1 << 28, &[0xf0, 0x10, 0x00, 0x00, 0x00]);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn test_encode_usize_64_bit() {
        test_encode_usize_impl((1 << 35) - 1, &[0xf7, 0xff, 0xff, 0xff, 0xff]);
        test_encode_usize_impl(1 << 35, &[0xf8, 0x08, 0x00, 0x00, 0x00, 0x00]);
        test_encode_usize_impl(
            (1 << 56) - 1,
            &[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        );
        test_encode_usize_impl(
            1 << 56,
            &[0xff, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        );
        test_encode_usize_impl(usize::MAX, &[0xff; 9]);
    }

    fn test_decode_usize_impl(encoding: &[u8], expected: usize) {
        let bytes = [encoding, &[0x42]].concat();
        let (value, length) = decode_usize(&bytes).unwrap();
        assert_eq!(value, expected);
        assert_eq!(length, encoding.len());
    }

    #[test]
    fn test_decode_usize() {
        test_decode_usize_impl(&[0x00], 0);
        test_decode_usize_impl(&[0x01], 1);
        test_decode_usize_impl(&[0x7f], 127);
        test_decode_usize_impl(&[0x80, 0x80], 128);
        test_decode_usize_impl(&[0x81, 0x2c], 300);
        test_decode_usize_impl(&[0xbf, 0xff], 16383);
        test_decode_usize_impl(&[0xc0, 0x40, 0x00], 16384);
        test_decode_usize_impl(&[0xdf, 0xff, 0xff], (1 << 21) - 1);
        test_decode_usize_impl(&[0xe0, 0x20, 0x00, 0x00], 1 << 21);
        test_decode_usize_impl(&[0xef, 0xff, 0xff, 0xff], (1 << 28) - 1);
        test_decode_usize_impl(&[0xf0, 0x10, 0x00, 0x00, 0x00], 1 << 28);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn test_decode_usize_64_bit() {
        test_decode_usize_impl(&[0xf7, 0xff, 0xff, 0xff, 0xff], (1 << 35) - 1);
        test_decode_usize_impl(&[0xf8, 0x08, 0x00, 0x00, 0x00, 0x00], 1 << 35);
        test_decode_usize_impl(
            &[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            (1 << 56) - 1,
        );
        test_decode_usize_impl(
            &[0xff, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            1 << 56,
        );
        test_decode_usize_impl(&[0xff; 9], usize::MAX);
    }

    #[test]
    fn test_decode_usize_non_minimal() {
        test_decode_usize_impl(&[0x80, 0x01], 1);
        test_decode_usize_impl(&[0xc0, 0x00, 0x7f], 127);
        test_decode_usize_impl(&[0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01], 1);
    }

    #[test]
    fn test_decode_consecutive_usizes() {
        let bytes = [0x05, 0x81, 0x2c, 0x00];
        let (first, first_length) = decode_usize(&bytes).unwrap();
        let (second, second_length) = decode_usize(&bytes[1..]).unwrap();
        let (third, third_length) = decode_usize(&bytes[3..]).unwrap();
        assert_eq!((first, first_length), (5, 1));
        assert_eq!((second, second_length), (300, 2));
        assert_eq!((third, third_length), (0, 1));
    }

    #[test]
    fn test_decode_usize_empty() {
        assert!(decode_usize(&[]).is_err());
    }

    fn test_decode_usize_truncated_impl(bytes: &[u8]) {
        assert!(decode_usize(bytes).is_err());
    }

    #[test]
    fn test_decode_usize_truncated() {
        test_decode_usize_truncated_impl(&[0x80]);
        test_decode_usize_truncated_impl(&[0xc0, 0x40]);
        test_decode_usize_truncated_impl(&[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        test_decode_usize_truncated_impl(&[0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    fn test_encode_field_element_impl<F: Field>(value: F, expected: &[u8]) {
        let mut bytes = vec![0x42];
        encode_field_element(value, &mut bytes);
        assert_eq!(bytes, [&[0x42], expected].concat());
    }

    #[test]
    fn test_encode_field_element() {
        test_encode_field_element_impl(KB::try_from(0u32).unwrap(), &[0x00, 0x00, 0x00, 0x00]);
        test_encode_field_element_impl(
            KB::try_from(0x01020304u32).unwrap(),
            &[0x01, 0x02, 0x03, 0x04],
        );
        test_encode_field_element_impl(KB::MAX, &[0x7f, 0x00, 0x00, 0x00]);
        test_encode_field_element_impl(Scalar::from(0u64), &[0x00; 32]);
        test_encode_field_element_impl(
            Scalar::from(0x0102u64),
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x01, 0x02,
            ],
        );
        test_encode_field_element_impl(
            Scalar::MAX,
            &[
                0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xfe, 0x06, 0x73, 0xdd, 0xf2, 0x9e, 0x9b, 0x55, 0x47, 0xc0, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
        );
        test_encode_field_element_impl(KB8::from(0u32), &[0x00; 31]);
        test_encode_field_element_impl(KB8::MAX, &KB8_MAX);
    }

    /// The 31-byte encoding of the largest KoalaBear^8 element, `0x7f000001^8 - 1`.
    const KB8_MAX: [u8; 31] = [
        0xf0, 0x6e, 0x44, 0x68, 0x2c, 0x2a, 0xa4, 0x40, 0xf5, 0xf2, 0x6a, 0x5a, 0xe1, 0x74, 0x90,
        0x05, 0x68, 0x74, 0x4c, 0xd6, 0x53, 0xc8, 0x06, 0xe4, 0x1c, 0x00, 0x03, 0xf8, 0x00, 0x00,
        0x00,
    ];

    fn test_decode_field_element_impl<F: Field>(encoding: &[u8], expected: F) {
        let bytes = [encoding, &[0x42]].concat();
        let (value, length) = decode_field_element::<F>(&bytes).unwrap();
        assert_eq!(value, expected);
        assert_eq!(length, encoding.len());
    }

    #[test]
    fn test_decode_field_element() {
        test_decode_field_element_impl(&[0x00, 0x00, 0x00, 0x00], KB::try_from(0u32).unwrap());
        test_decode_field_element_impl(
            &[0x01, 0x02, 0x03, 0x04],
            KB::try_from(0x01020304u32).unwrap(),
        );
        test_decode_field_element_impl(&[0x7f, 0x00, 0x00, 0x00], KB::MAX);
        test_decode_field_element_impl(&[0x00; 32], Scalar::from(0u64));
        test_decode_field_element_impl(
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x01, 0x02,
            ],
            Scalar::from(0x0102u64),
        );
        test_decode_field_element_impl(
            &[
                0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xfe, 0x06, 0x73, 0xdd, 0xf2, 0x9e, 0x9b, 0x55, 0x47, 0xc0, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            Scalar::MAX,
        );
        test_decode_field_element_impl(&[0x00; 31], KB8::from(0u32));
        test_decode_field_element_impl(&KB8_MAX, KB8::MAX);
    }

    fn test_decode_field_element_error_impl<F: Field>(bytes: &[u8]) {
        assert!(decode_field_element::<F>(bytes).is_err());
    }

    #[test]
    fn test_decode_field_element_truncated() {
        test_decode_field_element_error_impl::<KB>(&[]);
        test_decode_field_element_error_impl::<KB>(&[0x01, 0x02, 0x03]);
        test_decode_field_element_error_impl::<Scalar>(&[0x00; 31]);
        test_decode_field_element_error_impl::<KB8>(&[0x00; 30]);
    }

    #[test]
    fn test_decode_field_element_non_canonical() {
        // The moduli.
        test_decode_field_element_error_impl::<KB>(&[0x7f, 0x00, 0x00, 0x01]);
        test_decode_field_element_error_impl::<Scalar>(&[
            0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xfe, 0x06, 0x73, 0xdd, 0xf2, 0x9e, 0x9b, 0x55, 0x47, 0xc0, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01,
        ]);
        // All ones.
        test_decode_field_element_error_impl::<KB>(&[0xff; 4]);
        test_decode_field_element_error_impl::<KB8>(&[0xff; 31]);
    }

    #[test]
    #[ignore = "starkom-bluesky 3.8.0 panics on overflow in debug builds for some words above P"]
    fn test_decode_field_element_non_canonical_bluesky_all_ones() {
        test_decode_field_element_error_impl::<Scalar>(&[0xff; 32]);
    }
}
