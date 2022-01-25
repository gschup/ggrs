use bytemuck::Pod;

use crate::GGRSError;

// special thanks to james7132

pub(crate) fn encode<'a, T: Pod>(
    reference: T,
    pending_input: impl Iterator<Item = &'a T>,
) -> Vec<u8> {
    // first, do a XOR encoding to the reference input (will probably lead to a lot of same bits in sequence)
    let buf = delta_encode(reference, pending_input);
    // then, RLE encode the buffer (making use of the property mentioned above)
    bitfield_rle::encode(buf)
}

pub(crate) fn delta_encode<'a, T: Pod>(
    reference: T,
    pending_input: impl Iterator<Item = &'a T>,
) -> Vec<u8> {
    let ref_bytes = bytemuck::bytes_of(&reference);
    let (lower, upper) = pending_input.size_hint();
    let capacity = upper.unwrap_or(lower) * ref_bytes.len();
    let mut bytes = Vec::with_capacity(capacity);

    for input in pending_input {
        let input_bytes = bytemuck::bytes_of(input);
        assert_eq!(input_bytes.len(), ref_bytes.len());

        for (b1, b2) in ref_bytes.iter().zip(input_bytes.iter()) {
            bytes.push(b1 ^ b2);
        }
    }
    bytes
}

pub(crate) fn decode<T: Pod>(
    reference: T,
    data: &[u8],
) -> Result<Vec<T>, Box<dyn std::error::Error>> {
    // decode the RLE encoding first
    let buf = bitfield_rle::decode(data)?;

    // decode the delta-encoding
    delta_decode(reference, &buf)
}

pub(crate) fn delta_decode<T: Pod>(
    reference: T,
    data: &[u8],
) -> Result<Vec<T>, Box<dyn std::error::Error>> {
    let ref_bytes = bytemuck::bytes_of(&reference);
    assert!(data.len() % ref_bytes.len() == 0);
    let out_size = data.len() / ref_bytes.len();
    let mut output = Vec::with_capacity(out_size);

    for inp in 0..out_size {
        let mut buffer = vec![0u8; ref_bytes.len()];
        for i in 0..ref_bytes.len() {
            buffer[i] = ref_bytes[i] ^ data[ref_bytes.len() * inp + i];
        }
        output.push(*bytemuck::try_from_bytes::<T>(&buffer).map_err(|_| GGRSError::DecodingError)?);
    }

    Ok(output)
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod compression_tests {
    use super::*;

    use bytemuck::{Pod, Zeroable};

    #[repr(C)]
    #[derive(Copy, Clone, PartialEq, Pod, Zeroable)]
    struct TestInput {
        inp: u8,
    }

    #[test]
    fn test_encode_decode() {
        let ref_input = TestInput { inp: 2 };
        let inp0 = TestInput { inp: 0 };
        let inp1 = TestInput { inp: 1 };
        let inp2 = TestInput { inp: 2 };
        let inp3 = TestInput { inp: 3 };
        let inp4 = TestInput { inp: 4 };

        let pend_inp = vec![inp0, inp1, inp2, inp3, inp4];

        let encoded = encode(ref_input, pend_inp.iter());
        let decoded = decode(ref_input, &encoded).unwrap();

        assert!(pend_inp == decoded);
    }
}
