// special thanks to james7132

pub(crate) fn encode<'a>(
    reference: &[u8],
    pending_input: impl Iterator<Item = &'a Vec<u8>>,
) -> Vec<u8> {
    // first, do a XOR encoding to the reference input (will probably lead to a lot of same bits in sequence)
    let buf = delta_encode(reference, pending_input);
    // then, RLE encode the buffer (making use of the property mentioned above)
    bitfield_rle::encode(buf)
}

fn delta_encode<'a>(ref_bytes: &[u8], pending_input: impl Iterator<Item = &'a Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut base: Vec<u8> = ref_bytes.to_vec();

    for input in pending_input {
        // write the length of this input so the decoder can split correctly
        bytes.extend_from_slice(&(input.len() as u16).to_le_bytes());

        // XOR against the base up to the shorter of the two, append remainder as-is
        for (b1, b2) in base.iter().zip(input.iter()) {
            bytes.push(b1 ^ b2);
        }
        if input.len() > base.len() {
            bytes.extend_from_slice(&input[base.len()..]);
        }

        base = input.to_vec();
    }
    bytes
}

pub(crate) fn decode(
    reference: &[u8],
    data: &[u8],
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    // decode the RLE encoding first
    let buf = bitfield_rle::decode(data)?;

    // decode the delta-encoding
    delta_decode(reference, &buf)
}

fn delta_decode(
    ref_bytes: &[u8],
    data: &[u8],
) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut output = Vec::new();
    let mut pos = 0;
    let mut base: Vec<u8> = ref_bytes.to_vec();

    while pos < data.len() {
        // read the 2-byte length prefix
        if pos + 2 > data.len() {
            return Err("truncated length prefix".into());
        }
        let len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + len > data.len() {
            return Err("truncated input data".into());
        }
        let encoded = &data[pos..pos + len];
        pos += len;

        // XOR against the base up to the shorter of the two, append remainder as-is
        let mut decoded = encoded.to_vec();
        for (d, b) in decoded.iter_mut().zip(base.iter()) {
            *d ^= b;
        }

        base = decoded.clone();
        output.push(decoded);
    }

    Ok(output)
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod compression_tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let ref_input = vec![0, 0, 0, 1];
        let inp0: Vec<u8> = vec![0, 0, 1, 0];
        let inp1: Vec<u8> = vec![0, 0, 1, 1];
        let inp2: Vec<u8> = vec![0, 1, 0, 0];
        let inp3: Vec<u8> = vec![0, 1, 0, 1];
        let inp4: Vec<u8> = vec![0, 1, 1, 0];

        let pend_inp = vec![inp0, inp1, inp2, inp3, inp4];

        let encoded = encode(&ref_input, pend_inp.iter());
        let decoded = decode(&ref_input, &encoded).unwrap();

        assert!(pend_inp == decoded);
    }

    #[test]
    fn test_encode_decode_identical_to_reference() {
        let reference = vec![1, 2, 3, 4];
        let inputs = vec![reference.clone(), reference.clone()];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_single_input() {
        let reference = vec![0u8; 4];
        let inputs = vec![vec![1u8, 2, 3, 4]];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_all_zeros() {
        let reference = vec![0u8; 4];
        let inputs = vec![vec![0u8; 4], vec![0u8; 4], vec![0u8; 4]];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_variable_size() {
        // simulate an enum whose variants serialize to different sizes
        let reference = vec![0u8; 1];
        let inputs = vec![
            vec![1u8],                // 1 byte variant
            vec![2u8, 10, 20],        // 3 byte variant
            vec![1u8],                // back to 1 byte
            vec![3u8, 1, 2, 3, 4, 5], // 6 byte variant
        ];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_empty_inputs() {
        let reference = vec![1u8, 2, 3, 4];
        let inputs: Vec<Vec<u8>> = vec![];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_input_shorter_than_reference() {
        let reference = vec![1u8, 2, 3, 4];
        let inputs = vec![vec![0u8, 1], vec![5u8, 6, 7, 8]];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_encode_decode_input_size_alternating() {
        // each input is delta-encoded against the previous input, not the reference —
        // this verifies that the base advances correctly through the sequence
        let reference = vec![0u8; 2];
        let inputs = vec![
            vec![1u8, 2, 3, 4, 5, 6],
            vec![7u8, 8],
            vec![9u8, 10, 11, 12, 13, 14],
            vec![15u8, 16],
        ];
        let encoded = encode(&reference, inputs.iter());
        let decoded = decode(&reference, &encoded).unwrap();
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn test_decode_garbage_bytes_never_panics() {
        let reference = vec![0u8; 4];
        // pre-RLE-encode each garbage payload so bitfield_rle::decode succeeds and our
        // delta_decode layer receives the raw garbage — none of these should panic
        let cases: &[&[u8]] = &[
            &[],
            &[0xFF],
            &[0x00, 0x00],
            &[0xFF, 0xFF, 0xFF, 0xFF],
            &[0x01, 0x00, 0x00],
            &[0x00, 0x01, 0x00],
        ];
        for &garbage in cases {
            let rle_encoded = bitfield_rle::encode(garbage.to_vec());
            let _ = decode(&reference, &rle_encoded);
        }
    }

    #[test]
    fn test_decode_truncated_length_prefix_returns_error() {
        let reference = vec![0u8; 4];
        // only 1 byte — not enough for a length prefix
        let bad_data = bitfield_rle::encode(vec![0x01]);
        assert!(decode(&reference, &bad_data).is_err());
    }

    #[test]
    fn test_decode_truncated_input_data_returns_error() {
        let reference = vec![0u8; 4];
        // length prefix says 10 bytes but only 2 follow
        let mut raw = vec![];
        raw.extend_from_slice(&10u16.to_le_bytes());
        raw.extend_from_slice(&[0x01, 0x02]);
        let bad_data = bitfield_rle::encode(raw);
        assert!(decode(&reference, &bad_data).is_err());
    }
}
