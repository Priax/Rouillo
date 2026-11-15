use bitcode::Encode;

thread_local! {
    static ENCODER: std::cell::RefCell<(bitcode::Buffer, Vec<u8>)> = std::cell::RefCell::default();
}

/// The message compressed, after its uncompressed size. The encoder and the
/// compression scratch are kept from one message to the next: the output is
/// the only allocation.
pub fn encode<T: Encode + ?Sized>(msg: &T) -> Vec<u8> {
    ENCODER.with_borrow_mut(|(buffer, scratch)| {
        let raw = buffer.encode(msg);
        scratch.resize(lz4_flex::block::get_maximum_output_size(raw.len()), 0);
        let len = lz4_flex::block::compress_into(raw, scratch).expect("sized for the worst case");
        let mut out = Vec::with_capacity(4 + len);
        out.extend_from_slice(&u32::try_from(raw.len()).unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&scratch[..len]);
        out
    })
}

pub const MAX_DECODED_SIZE: usize = 1 << 20;

pub fn decode<T: bitcode::DecodeOwned>(bytes: &[u8]) -> Option<T> {
    decode_bounded(bytes, MAX_DECODED_SIZE)
}

/// `decode`, for what may grow past a message's size, as a replay does.
pub fn decode_bounded<T: bitcode::DecodeOwned>(bytes: &[u8], max_size: usize) -> Option<T> {
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    if size > max_size {
        return None;
    }
    let raw = lz4_flex::block::decompress_size_prepended(bytes).ok()?;
    bitcode::decode(&raw).ok()
}

/// On a WebTransport stream, each message comes after its length in these
/// bytes, little-endian.
pub const FRAME_HEADER: usize = 4;

pub fn frame_header(len: usize) -> [u8; FRAME_HEADER] {
    u32::try_from(len).unwrap_or(u32::MAX).to_le_bytes()
}

pub fn frame_len(header: [u8; FRAME_HEADER]) -> usize {
    u32::from_le_bytes(header) as usize
}
