//! Password-protect a finished PDF. Pure and synchronous, so the native server and the
//! WebAssembly build share it.

use lopdf::{
    Document, Object,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};

/// Encrypt `pdf` (RC4 128-bit) with `password` as both the open and the owner password.
pub fn encrypt(pdf: &[u8], password: &str) -> Result<Vec<u8>, String> {
    let err = |e: lopdf::Error| e.to_string();
    let mut doc = Document::load_mem(pdf).map_err(err)?;
    ensure_file_id(&mut doc, pdf);
    let version = EncryptionVersion::V2 {
        document: &doc,
        owner_password: password,
        user_password: password,
        permissions: Permissions::all(),
        key_length: 128,
    };
    let state: EncryptionState = version.try_into().map_err(err)?;
    doc.encrypt(&state).map_err(err)?;
    let mut out = Vec::with_capacity(pdf.len() + 4096);
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Encryption keys are derived from the trailer `/ID`; add one (the MD5 of the document) if the
/// producer didn't. No randomness, so it works in the browser too.
fn ensure_file_id(doc: &mut Document, pdf: &[u8]) {
    if doc.trailer.get(b"ID").is_err() {
        let id = md5::compute(pdf).0.to_vec();
        let id = Object::String(id, lopdf::StringFormat::Hexadecimal);
        doc.trailer.set("ID", vec![id.clone(), id]);
    }
}
