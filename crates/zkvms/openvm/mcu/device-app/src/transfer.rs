use alloc::{format, string::String, string::ToString, vec::Vec};
use core::fmt;

pub const VERIFIER_KEY_PACKET_KIND: u8 = 1;
pub const PROOF_ENVELOPE_PACKET_KIND: u8 = 2;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TransferState {
    pub expected_key_len: usize,
    pub expected_proof_len: usize,
    pub proof_sha: String,
    pub key_bytes: Vec<u8>,
    pub proof_bytes: Vec<u8>,
    pub verifying: bool,
    pub receive_started_us: i64,
    pub receive_done_us: i64,
    pub verify_started_us: i64,
    pub verify_done_us: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferStart {
    pub expected_key_len: usize,
    pub expected_proof_len: usize,
    pub proof_sha: String,
    pub proof_sha_prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyArtifacts {
    pub key_bytes: Vec<u8>,
    pub proof_bytes: Vec<u8>,
    pub proof_sha: String,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferError {
    InvalidStart,
    AlreadyVerifying,
    IncompleteTransfer,
    ShortPacket,
    BadPacketKind(u8),
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStart => formatter.write_str("invalid START"),
            Self::AlreadyVerifying => formatter.write_str("already verifying"),
            Self::IncompleteTransfer => formatter.write_str("incomplete transfer"),
            Self::ShortPacket => formatter.write_str("short packet"),
            Self::BadPacketKind(kind) => write!(formatter, "bad packet kind {kind}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TransferError {}

impl TransferState {
    pub fn start_from_command(
        &mut self,
        command: &str,
        now_us: i64,
    ) -> Result<TransferStart, TransferError> {
        let rest = command
            .trim()
            .strip_prefix("START ")
            .ok_or(TransferError::InvalidStart)?;
        let start = parse_start(rest)?;
        self.start(
            start.expected_key_len,
            start.expected_proof_len,
            start.proof_sha.clone(),
            now_us,
        );
        Ok(start)
    }

    pub fn start(
        &mut self,
        expected_key_len: usize,
        expected_proof_len: usize,
        proof_sha: String,
        now_us: i64,
    ) {
        self.expected_key_len = expected_key_len;
        self.expected_proof_len = expected_proof_len;
        self.proof_sha = proof_sha;
        self.key_bytes = Vec::with_capacity(expected_key_len);
        self.proof_bytes = Vec::with_capacity(expected_proof_len);
        self.verifying = false;
        self.receive_started_us = now_us;
        self.receive_done_us = 0;
        self.verify_started_us = 0;
        self.verify_done_us = 0;
    }

    pub fn ingest_packet(
        &mut self,
        packet: &[u8],
        now_us: i64,
    ) -> Result<Option<String>, TransferError> {
        if packet.len() < 4 {
            return Err(TransferError::ShortPacket);
        }

        let kind = packet[0];
        let payload = &packet[4..];
        match kind {
            VERIFIER_KEY_PACKET_KIND => self.key_bytes.extend_from_slice(payload),
            PROOF_ENVELOPE_PACKET_KIND => self.proof_bytes.extend_from_slice(payload),
            _ => return Err(TransferError::BadPacketKind(kind)),
        }

        if self.transfer_complete() && self.receive_done_us == 0 {
            self.receive_done_us = now_us;
            return Ok(Some(self.timing_detail(now_us)));
        }

        Ok(None)
    }

    pub fn begin_verify(&mut self, now_us: i64) -> Result<VerifyArtifacts, TransferError> {
        if self.verifying {
            return Err(TransferError::AlreadyVerifying);
        }
        if !self.transfer_complete() {
            return Err(TransferError::IncompleteTransfer);
        }

        self.verifying = true;
        self.verify_started_us = now_us;
        Ok(VerifyArtifacts {
            key_bytes: self.key_bytes.clone(),
            proof_bytes: self.proof_bytes.clone(),
            proof_sha: self.proof_sha.clone(),
            detail: self.timing_detail(now_us),
        })
    }

    pub fn mark_verify_started(&mut self, now_us: i64) -> String {
        if self.verify_started_us == 0 {
            self.verify_started_us = now_us;
        }
        self.timing_detail(now_us)
    }

    pub fn mark_verify_done(&mut self, now_us: i64) -> String {
        self.verify_done_us = now_us;
        self.timing_detail(now_us)
    }

    pub fn finish_verify(&mut self) {
        self.verifying = false;
    }

    pub fn is_receiving(&self) -> bool {
        self.receive_started_us > 0 && self.receive_done_us == 0 && !self.verifying
    }

    pub fn receiving_detail(&self, now_us: i64) -> Option<String> {
        self.is_receiving().then(|| self.timing_detail(now_us))
    }

    pub fn timing_detail(&self, now_us: i64) -> String {
        let receive_end = if self.receive_done_us > 0 {
            self.receive_done_us
        } else {
            now_us
        };
        let receive_ms = elapsed_ms(self.receive_started_us, receive_end);
        let received_bytes = self.total_received_bytes();
        let expected_bytes = self.total_expected_bytes();
        let receive_percent = self.receive_percent();
        let verify_ms = if self.verify_started_us > 0 {
            let verify_end = if self.verify_done_us > 0 {
                self.verify_done_us
            } else {
                now_us
            };
            elapsed_ms(self.verify_started_us, verify_end)
        } else {
            0
        };

        if self.verify_started_us > 0 {
            format!(
                "rx {}% {}/{}B {}ms verify {}ms sha {}",
                receive_percent,
                received_bytes,
                expected_bytes,
                receive_ms,
                verify_ms,
                proof_sha_prefix(&self.proof_sha)
            )
        } else {
            format!(
                "rx {}% {}/{}B {}ms sha {}",
                receive_percent,
                received_bytes,
                expected_bytes,
                receive_ms,
                proof_sha_prefix(&self.proof_sha)
            )
        }
    }

    pub fn total_expected_bytes(&self) -> usize {
        self.expected_key_len.saturating_add(self.expected_proof_len)
    }

    pub fn total_received_bytes(&self) -> usize {
        self.key_bytes.len().saturating_add(self.proof_bytes.len())
    }

    pub fn receive_percent(&self) -> usize {
        let expected = self.total_expected_bytes();
        if expected == 0 {
            0
        } else {
            ((self.total_received_bytes().min(expected) * 100) / expected).min(100)
        }
    }

    fn transfer_complete(&self) -> bool {
        self.expected_key_len != 0
            && self.expected_proof_len != 0
            && self.key_bytes.len() >= self.expected_key_len
            && self.proof_bytes.len() >= self.expected_proof_len
    }
}

pub fn parse_start(rest: &str) -> Result<TransferStart, TransferError> {
    let mut parts = rest.split_whitespace();
    let expected_key_len = parts
        .next()
        .ok_or(TransferError::InvalidStart)?
        .parse()
        .map_err(|_| TransferError::InvalidStart)?;
    let expected_proof_len = parts
        .next()
        .ok_or(TransferError::InvalidStart)?
        .parse()
        .map_err(|_| TransferError::InvalidStart)?;
    let proof_sha = parts.next().unwrap_or("unknown").to_string();
    let proof_sha_prefix = proof_sha_prefix(&proof_sha).to_string();
    Ok(TransferStart {
        expected_key_len,
        expected_proof_len,
        proof_sha,
        proof_sha_prefix,
    })
}

pub fn proof_sha_prefix(proof_sha: &str) -> &str {
    proof_sha.get(..16).unwrap_or(proof_sha)
}

pub fn elapsed_ms(start_us: i64, end_us: i64) -> u64 {
    if start_us <= 0 || end_us <= start_us {
        0
    } else {
        ((end_us - start_us) / 1_000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_accumulates_artifacts_and_reports_timing() {
        let mut state = TransferState::default();
        let start = state
            .start_from_command("START 3 2 abcdef0123456789", 1_000)
            .unwrap();
        assert_eq!(start.proof_sha_prefix, "abcdef0123456789");

        assert_eq!(state.ingest_packet(&[1, 0, 0, 0, 1, 2, 3], 2_000), Ok(None));
        let received = state.ingest_packet(&[2, 0, 0, 1, 4, 5], 4_000).unwrap();
        assert_eq!(
            received,
            Some("rx 100% 5/5B 3ms sha abcdef0123456789".to_string())
        );

        let artifacts = state.begin_verify(5_000).unwrap();
        assert_eq!(artifacts.key_bytes, vec![1, 2, 3]);
        assert_eq!(artifacts.proof_bytes, vec![4, 5]);
        assert_eq!(
            state.mark_verify_done(9_000),
            "rx 100% 5/5B 3ms verify 4ms sha abcdef0123456789"
        );
    }

    #[test]
    fn rejects_short_and_unknown_packets() {
        let mut state = TransferState::default();
        assert_eq!(
            state.ingest_packet(&[1, 2, 3], 0),
            Err(TransferError::ShortPacket)
        );
        assert_eq!(
            state.ingest_packet(&[9, 0, 0, 0], 0),
            Err(TransferError::BadPacketKind(9))
        );
    }

    #[test]
    fn reports_receiving_progress_without_new_packets() {
        let mut state = TransferState::default();
        state
            .start_from_command("START 3 2 abcdef0123456789", 1_000)
            .unwrap();

        assert_eq!(
            state.receiving_detail(1_000),
            Some("rx 0% 0/5B 0ms sha abcdef0123456789".to_string())
        );

        state.ingest_packet(&[1, 0, 0, 0, 1, 2, 3], 2_000).unwrap();
        assert_eq!(
            state.receiving_detail(6_000),
            Some("rx 60% 3/5B 5ms sha abcdef0123456789".to_string())
        );
    }
}
