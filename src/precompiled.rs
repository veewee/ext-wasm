//! The file format of precompiled artifacts: a small envelope around the bytes
//! wasmtime's `serialize` produces.
//!
//! The envelope exists because wasmtime drops custom sections, which
//! `Module::customSections()` reads, and because a checksum catches a corrupted
//! artifact before wasmtime runs code from it. All integers are little-endian:
//! magic, format version (u8), kind (u8), CRC32 of everything after it (u32),
//! section count (u32), per section a u32-length name and a u32-length data,
//! payload.

const MAGIC: &[u8; 8] = b"\0phpwasm";
const VERSION: u8 = 1;
/// Where the bytes the checksum covers start: right after the checksum.
const CHECKED_FROM: usize = MAGIC.len() + 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Module = 0,
    Component = 1,
}

impl Kind {
    fn method(self) -> &'static str {
        match self {
            Kind::Module => "Wasm\\Serializer::deserializeModule()",
            Kind::Component => "Wasm\\Serializer::deserializeComponent()",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Module => "module",
            Kind::Component => "component",
        }
    }
}

#[derive(Debug)]
pub struct Artifact<'a> {
    pub sections: Vec<(String, Vec<u8>)>,
    pub payload: &'a [u8],
}

pub fn encode(kind: Kind, sections: &[(String, Vec<u8>)], payload: Vec<u8>) -> Vec<u8> {
    let header_len = CHECKED_FROM
        + 4
        + sections
            .iter()
            .map(|(name, data)| 8 + name.len() + data.len())
            .sum::<usize>();
    let mut out = Vec::with_capacity(header_len + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(kind as u8);
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&length(sections.len()));
    for (name, data) in sections {
        out.extend_from_slice(&length(name.len()));
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&length(data.len()));
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&payload);
    let crc = crc32fast::hash(&out[CHECKED_FROM..]);
    out[CHECKED_FROM - 4..CHECKED_FROM].copy_from_slice(&crc.to_le_bytes());
    out
}

fn length(len: usize) -> [u8; 4] {
    u32::try_from(len)
        .expect("a custom section or count fits in a wasm binary, so in u32")
        .to_le_bytes()
}

/// Checks the envelope of `bytes` and that it holds a `kind`.
pub fn decode(bytes: &[u8], kind: Kind) -> Result<Artifact<'_>, String> {
    let rest = bytes.strip_prefix(MAGIC).ok_or_else(|| {
        "not a precompiled artifact from Wasm\\Serializer; compile the .wasm with new Wasm\\Module() or new Wasm\\Component\\Component() and serialize that".to_string()
    })?;
    let mut reader = Reader(rest);
    let version = reader.u8()?;
    if version != VERSION {
        return Err(format!(
            "the artifact has format version {version}, this ext-wasm reads version {VERSION}; rebuild the artifact"
        ));
    }
    let found = match reader.u8()? {
        0 => Kind::Module,
        1 => Kind::Component,
        other => return Err(format!("the artifact has an unknown kind {other}")),
    };
    if found != kind {
        return Err(format!(
            "this is a {} artifact, use {}",
            found.name(),
            found.method()
        ));
    }
    let crc = reader.u32()?;
    if crc32fast::hash(reader.0) != crc {
        return Err("the artifact is corrupted: its checksum does not match".to_string());
    }
    let count = reader.u32()?;
    let mut sections = Vec::new();
    for _ in 0..count {
        let name = reader.bytes()?;
        let name = std::str::from_utf8(name)
            .map_err(|_| "the artifact is corrupted: a section name is not UTF-8".to_string())?;
        sections.push((name.to_string(), reader.bytes()?.to_vec()));
    }
    Ok(Artifact {
        sections,
        payload: reader.0,
    })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        if self.0.len() < len {
            return Err("the artifact is truncated".to_string());
        }
        let (head, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("take returned 4 bytes"),
        ))
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let len = self.u32()? as usize;
        self.take(len)
    }
}

/// The error for an artifact wasmtime refuses, with what to do about it.
pub fn incompatible(err: impl std::fmt::Display) -> String {
    format!(
        "{err:#} (rebuild the artifact on this host, or on one with the same OS, at least its CPU features and the same ext-wasm version)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        encode(
            Kind::Module,
            &[("meta".to_string(), b"da\0ta".to_vec())],
            b"payload".to_vec(),
        )
    }

    #[test]
    fn round_trips() {
        let bytes = sample();
        let artifact = decode(&bytes, Kind::Module).unwrap();
        assert_eq!(artifact.payload, b"payload");
        assert_eq!(
            artifact.sections,
            [("meta".to_string(), b"da\0ta".to_vec())]
        );
    }

    #[test]
    fn every_truncation_is_an_error() {
        let bytes = sample();
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len], Kind::Module).is_err(), "length {len}");
        }
    }

    /// Recomputes the checksum after a test changed bytes it covers.
    fn reseal(mut bytes: Vec<u8>) -> Vec<u8> {
        let crc = crc32fast::hash(&bytes[CHECKED_FROM..]);
        bytes[CHECKED_FROM - 4..CHECKED_FROM].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    #[test]
    fn a_changed_byte_anywhere_after_the_checksum_is_corruption() {
        let bytes = sample();
        for at in CHECKED_FROM..bytes.len() {
            let mut changed = bytes.clone();
            changed[at] ^= 0xff;
            assert_eq!(
                decode(&changed, Kind::Module).unwrap_err(),
                "the artifact is corrupted: its checksum does not match",
                "byte {at}"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_corruption() {
        let mut bytes = sample();
        bytes.push(0);
        assert!(
            decode(&bytes, Kind::Module)
                .unwrap_err()
                .contains("corrupted")
        );
    }

    #[test]
    fn another_format_version_is_an_error() {
        let mut bytes = sample();
        bytes[8] = 2;
        assert!(
            decode(&bytes, Kind::Module)
                .unwrap_err()
                .contains("format version 2")
        );
    }

    #[test]
    fn an_unknown_kind_is_an_error() {
        let mut bytes = sample();
        bytes[9] = 9;
        assert_eq!(
            decode(&bytes, Kind::Module).unwrap_err(),
            "the artifact has an unknown kind 9"
        );
    }

    #[test]
    fn a_section_name_that_is_not_utf8_is_an_error() {
        let mut bytes = sample();
        bytes[22] = 0xff;
        assert!(
            decode(&reseal(bytes), Kind::Module)
                .unwrap_err()
                .contains("not UTF-8")
        );
    }

    #[test]
    fn huge_lengths_are_truncated_not_a_panic() {
        for at in [14, 18, 26] {
            let mut bytes = sample();
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(
                decode(&reseal(bytes), Kind::Module).unwrap_err(),
                "the artifact is truncated",
                "length at {at}"
            );
        }
    }

    #[test]
    fn the_kind_is_checked() {
        let bytes = sample();
        assert!(
            decode(&bytes, Kind::Component)
                .unwrap_err()
                .contains("deserializeModule()")
        );
    }
}
