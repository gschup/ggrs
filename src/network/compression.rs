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

pub(crate) fn delta_encode<'a>(
    ref_bytes: &[u8],
    pending_input: impl Iterator<Item = &'a Vec<u8>>,
) -> Vec<u8> {
    let (lower, upper) = pending_input.size_hint();
    let capacity = upper.unwrap_or(lower) * ref_bytes.len();
    let mut bytes = Vec::with_capacity(capacity);

    for input in pending_input {
        let input_bytes = input;
        assert_eq!(input_bytes.len(), ref_bytes.len());

        for (b1, b2) in ref_bytes.iter().zip(input_bytes.iter()) {
            bytes.push(b1 ^ b2);
        }
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
    Ok(delta_decode(reference, &buf))
}

pub(crate) fn delta_decode(ref_bytes: &[u8], data: &[u8]) -> Vec<Vec<u8>> {
    assert!(data.len() % ref_bytes.len() == 0);
    let out_size = data.len() / ref_bytes.len();
    let mut output = Vec::with_capacity(out_size);

    for inp in 0..out_size {
        let mut buffer = vec![0u8; ref_bytes.len()];
        for i in 0..ref_bytes.len() {
            buffer[i] = ref_bytes[i] ^ data[ref_bytes.len() * inp + i];
        }
        output.push(buffer);
    }

    output
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
}
