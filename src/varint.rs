use anyhow::Result;

pub fn encode_usize(value: usize, bytes: &mut [u8]) {
    // TODO
}

pub fn decode_usize(bytes: &[u8]) -> Result<usize> {
    // TODO
}

pub fn encode_field<F: Field>(value: F, bytes: &mut [u8]) {
    // TODO
}

pub fn decode_field<F: Field>(bytes: &[u8]) -> Result<F> {
    // TODO
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO
}
