use crate::GameInput;
use bitfield_rle;

pub(crate) fn encode<'a>(
    reference: &GameInput,
    pending_input: impl Iterator<Item = &'a GameInput>,
) -> Vec<u8> {
    // first, do a XOR encoding to the reference input (will probably lead to a lot of same bits in sequence)
    let buf = delta_encode(reference, pending_input);
    // then, RLE encode the buffer (making use of the property mentioned above)
    bitfield_rle::encode(buf)
}

pub(crate) fn delta_encode<'a>(
    reference: &GameInput,
    pending_input: impl Iterator<Item = &'a GameInput>,
) -> Vec<u8> {
    let ref_bytes = reference.input();
    let (lower, upper) = pending_input.size_hint();
    let capacity = upper.unwrap_or(lower) * reference.size;
    let mut bytes = Vec::with_capacity(capacity);

    for (i, input) in pending_input.enumerate() {
        assert!(input.size == reference.size);
        assert!(input.frame == reference.frame + i as i32 + 1);
        let input_bytes = input.input();
        for (b1, b2) in ref_bytes.iter().zip(input_bytes.iter()) {
            bytes.push(b1 ^ b2);
        }
    }
    bytes
}

pub(crate) fn decode(
    reference: &GameInput,
    data: impl AsRef<[u8]>,
) -> Result<Vec<GameInput>, Box<dyn std::error::Error>> {
    // decode the RLE encoding first
    let buf = bitfield_rle::decode(data)?;

    // decode the delta-encoding
    Ok(delta_decode(reference, buf))
}

pub(crate) fn delta_decode(reference: &GameInput, data: Vec<u8>) -> Vec<GameInput> {
    assert!(data.len() % reference.size == 0);
    let out_size = data.len() / reference.size;
    let mut output = Vec::with_capacity(out_size);

    for inp in 0..out_size {
        let mut game_input = GameInput::new(reference.frame + inp as i32 + 1, None, reference.size);
        for (i, byte) in reference.input().iter().enumerate() {
            game_input.buffer[i] = byte ^ data[reference.size * inp + i];
        }
        output.push(game_input);
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
        let mut ref_input = GameInput::new(5, None, 4);
        ref_input.buffer[3] = 1;
        let inp0 = GameInput::new(6, None, 4);
        let inp1 = GameInput::new(7, None, 4);
        let inp2 = GameInput::new(8, None, 4);
        let inp3 = GameInput::new(9, None, 4);
        let inp4 = GameInput::new(10, None, 4);

        let pend_inp = vec![inp0, inp1, inp2, inp3, inp4];

        let encoded = encode(&ref_input, pend_inp.iter());
        let decoded = decode(&ref_input, encoded).unwrap();

        assert!(pend_inp == decoded);
    }
}
