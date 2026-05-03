use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use serde::{Deserialize, Serialize};

pub const FRAME_MAGIC: [u8; 2] = *b"OV";
pub const FRAME_HEADER_LEN: usize = 7;
pub const PROTOCOL_VERSION: u16 = 1;
pub const OPENVM_EVM_HALO2_PROOF_DATA_LEN: usize = (12 + 43) * 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum ProofKind {
    OpenVmEvmHalo2 = 1,
    OpenVmStark = 2,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum FrameType {
    ProofEnvelope = 1,
    VerifierKey = 2,
    VerificationResponse = 3,
    ErrorFrame = 255,
}

impl TryFrom<u8> for FrameType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ProofEnvelope),
            2 => Ok(Self::VerifierKey),
            3 => Ok(Self::VerificationResponse),
            255 => Ok(Self::ErrorFrame),
            _ => Err(ProtocolError::UnknownFrameType(value)),
        }
    }
}

impl From<FrameType> for u8 {
    fn from(value: FrameType) -> Self {
        value as u8
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofEnvelope {
    pub protocol_version: u16,
    pub proof_kind: ProofKind,
    pub openvm_version: String,
    pub verifier_key_id: [u8; 32],
    pub app_exe_commit: [u8; 32],
    pub app_vm_commit: [u8; 32],
    pub user_public_values: Vec<u8>,
    pub proof_data: Vec<u8>,
    pub metadata: Vec<u8>,
}

impl ProofEnvelope {
    pub fn new_evm_halo2(
        openvm_version: String,
        verifier_key_id: [u8; 32],
        app_exe_commit: [u8; 32],
        app_vm_commit: [u8; 32],
        user_public_values: Vec<u8>,
        proof_data: Vec<u8>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proof_kind: ProofKind::OpenVmEvmHalo2,
            openvm_version,
            verifier_key_id,
            app_exe_commit,
            app_vm_commit,
            user_public_values,
            proof_data,
            metadata: Vec::new(),
        }
    }

    pub fn new_stark(
        openvm_version: String,
        verifier_key_id: [u8; 32],
        app_exe_commit: [u8; 32],
        app_vm_commit: [u8; 32],
        user_public_values: Vec<u8>,
        proof_json: Vec<u8>,
        verification_baseline_json: Vec<u8>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proof_kind: ProofKind::OpenVmStark,
            openvm_version,
            verifier_key_id,
            app_exe_commit,
            app_vm_commit,
            user_public_values,
            proof_data: proof_json,
            metadata: verification_baseline_json,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifierKey {
    pub protocol_version: u16,
    pub proof_kind: ProofKind,
    pub key_id: [u8; 32],
    pub payload: Vec<u8>,
}

impl VerifierKey {
    pub fn new(proof_kind: ProofKind, key_id: [u8; 32], payload: Vec<u8>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proof_kind,
            key_id,
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerificationReport {
    pub protocol_version: u16,
    pub proof_kind: ProofKind,
    pub verified: bool,
    pub public_values_len: u32,
}

impl VerificationReport {
    pub fn new(proof_kind: ProofKind, verified: bool, public_values_len: usize) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            proof_kind,
            verified,
            public_values_len: public_values_len as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    BadMagic,
    Truncated,
    LengthMismatch,
    PayloadTooLarge,
    UnknownFrameType(u8),
    Encode,
    Decode,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => formatter.write_str("bad frame magic"),
            Self::Truncated => formatter.write_str("truncated frame"),
            Self::LengthMismatch => formatter.write_str("frame length mismatch"),
            Self::PayloadTooLarge => formatter.write_str("payload too large"),
            Self::UnknownFrameType(frame_type) => {
                write!(formatter, "unknown frame type {frame_type}")
            }
            Self::Encode => formatter.write_str("message encoding failed"),
            Self::Decode => formatter.write_str("message decoding failed"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ProtocolError {}

pub struct FrameView<'payload> {
    pub frame_type: FrameType,
    pub payload: &'payload [u8],
}

pub fn encode_message<T: Serialize>(message: &T) -> Result<Vec<u8>, ProtocolError> {
    postcard::to_allocvec(message).map_err(|_| ProtocolError::Encode)
}

pub fn decode_message<'payload, T: Deserialize<'payload>>(
    payload: &'payload [u8],
) -> Result<T, ProtocolError> {
    postcard::from_bytes(payload).map_err(|_| ProtocolError::Decode)
}

pub fn encode_frame<T: Serialize>(
    frame_type: FrameType,
    message: &T,
) -> Result<Vec<u8>, ProtocolError> {
    let payload = encode_message(message)?;
    let payload_len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| ProtocolError::PayloadTooLarge)?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&FRAME_MAGIC);
    frame.push(frame_type.into());
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<FrameView<'_>, ProtocolError> {
    if frame.len() < FRAME_HEADER_LEN {
        return Err(ProtocolError::Truncated);
    }
    if frame[..2] != FRAME_MAGIC {
        return Err(ProtocolError::BadMagic);
    }

    let frame_type = FrameType::try_from(frame[2])?;
    let payload_len = u32::from_le_bytes(frame[3..7].try_into().expect("fixed width")) as usize;
    if frame.len() != FRAME_HEADER_LEN + payload_len {
        return Err(ProtocolError::LengthMismatch);
    }

    Ok(FrameView {
        frame_type,
        payload: &frame[FRAME_HEADER_LEN..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let proof = ProofEnvelope::new_evm_halo2(
            "v2.0".into(),
            [7; 32],
            [1; 32],
            [2; 32],
            vec![42],
            vec![3; OPENVM_EVM_HALO2_PROOF_DATA_LEN],
        );

        let frame = encode_frame(FrameType::ProofEnvelope, &proof).unwrap();
        let view = decode_frame(&frame).unwrap();
        assert_eq!(view.frame_type, FrameType::ProofEnvelope);

        let decoded: ProofEnvelope = decode_message(view.payload).unwrap();
        assert_eq!(decoded, proof);
    }
}
