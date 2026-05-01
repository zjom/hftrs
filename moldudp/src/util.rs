#[inline]
pub(crate) fn pad_session(s: &str) -> [u8; 10] {
    let mut out = [b' '; 10];
    let bytes = s.as_bytes();
    let n = bytes.len().min(10);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}
