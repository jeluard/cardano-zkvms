#![allow(dead_code)]

use alloc::{format, string::String, vec::Vec};

use openvm_mcu_device_app::{
    transfer::{
        TransferError, TransferState, VerifyArtifacts, PROOF_ENVELOPE_PACKET_KIND,
        VERIFIER_KEY_PACKET_KIND,
    },
    ProofProbe,
};

pub const DEVICE_NAME: &str = "ZKMCU";
pub const SERVICE_UUID: &str = "7b7c0001-78f1-4f9a-8b29-6f1f1d95a100";
pub const CONTROL_UUID: &str = "7b7c0002-78f1-4f9a-8b29-6f1f1d95a100";
pub const DATA_UUID: &str = "7b7c0003-78f1-4f9a-8b29-6f1f1d95a100";
pub const STATUS_UUID: &str = "7b7c0004-78f1-4f9a-8b29-6f1f1d95a100";

#[derive(Clone, Debug)]
pub enum Event {
    Status { status: String, detail: String },
    Verified { probe: ProofProbe, detail: String },
}

#[derive(Default)]
pub struct ProofApp {
    transfer: TransferState,
}

impl ProofApp {
    pub fn ready(&self) -> Event {
        Event::Status {
            status: String::from("ready"),
            detail: ble_detail(),
        }
    }

    pub fn connected(&self, now_us: i64) -> Event {
        if self.transfer.verifying {
            Event::Status {
                status: String::from("verifying halo2"),
                detail: self.transfer.timing_detail(now_us),
            }
        } else if self.transfer.is_receiving() {
            Event::Status {
                status: String::from("receiving"),
                detail: self.transfer.timing_detail(now_us),
            }
        } else {
            self.ready()
        }
    }

    pub fn control_write(&mut self, command: &str, now_us: i64) -> Event {
        let command = command.trim_matches('\0').trim();
        if command.starts_with("START ") {
            match self.transfer.start_from_command(command, now_us) {
                Ok(_) => Event::Status {
                    status: String::from("receiving"),
                    detail: self.transfer.timing_detail(now_us),
                },
                Err(error) => error_event(error),
            }
        } else if command.trim() == "COMMIT" {
            self.commit(now_us)
        } else {
            Event::Status {
                status: String::from("error unknown command"),
                detail: String::from("expected START or COMMIT"),
            }
        }
    }

    pub fn data_write(&mut self, packet: &[u8], now_us: i64) -> Event {
        match self.transfer.ingest_packet(packet, now_us) {
            Ok(Some(detail)) => Event::Status {
                status: String::from("received"),
                detail,
            },
            Ok(None) => Event::Status {
                status: String::from("receiving"),
                detail: self.transfer.timing_detail(now_us),
            },
            Err(error) => error_event(error),
        }
    }

    pub fn self_test(&mut self, key: &[u8], proof: &[u8], proof_sha: &str, now_us: i64) -> Event {
        let start = format!("START {} {} {}", key.len(), proof.len(), proof_sha);
        let _ = self.control_write(&start, now_us);
        let _ = self.data_write(&packet(VERIFIER_KEY_PACKET_KIND, key), now_us + 1_000);
        let _ = self.data_write(&packet(PROOF_ENVELOPE_PACKET_KIND, proof), now_us + 2_000);
        self.control_write("COMMIT", now_us + 3_000)
    }

    pub fn is_receiving(&self) -> bool {
        self.transfer.is_receiving()
    }

    pub fn receiving_progress(&self, now_us: i64) -> Option<Event> {
        self.transfer.receiving_detail(now_us).map(|detail| Event::Status {
            status: String::from("receiving"),
            detail,
        })
    }

    pub fn receive_percent(&self) -> usize {
        self.transfer.receive_percent()
    }

    pub fn is_verifying(&self) -> bool {
        self.transfer.verifying
    }

    pub fn begin_commit(&mut self, now_us: i64) -> Result<VerifyArtifacts, Event> {
        self.transfer.begin_verify(now_us).map_err(error_event)
    }

    pub fn complete_commit(&mut self, result: Result<ProofProbe, ()>, now_us: i64) -> Event {
        let detail = self.transfer.mark_verify_done(now_us);
        self.transfer.finish_verify();
        match result {
            Ok(probe) => Event::Verified { probe, detail },
            Err(()) => Event::Status {
                status: String::from("error decode failed"),
                detail,
            },
        }
    }

    pub fn cancel_commit(&mut self) {
        self.transfer.finish_verify();
    }

    fn commit(&mut self, now_us: i64) -> Event {
        let artifacts = match self.transfer.begin_verify(now_us) {
            Ok(artifacts) => artifacts,
            Err(error) => return error_event(error),
        };
        let _ = self.transfer.mark_verify_started(now_us);
        fn noop_step(_: u8, _: &'static str) {}
        let result = openvm_mcu_device_app::verify_received_artifacts(
            &artifacts.key_bytes,
            &artifacts.proof_bytes,
            artifacts.proof_sha,
            noop_step,
        );
        let detail = self.transfer.mark_verify_done(crate::now_us());
        self.transfer.finish_verify();
        match result {
            Ok(probe) => Event::Verified { probe, detail },
            Err(()) => Event::Status {
                status: String::from("error decode failed"),
                detail,
            },
        }
    }
}

fn ble_detail() -> String {
    format!(
        "{} svc={} ctl={} data={} stat={}",
        DEVICE_NAME, SERVICE_UUID, CONTROL_UUID, DATA_UUID, STATUS_UUID
    )
}

fn error_event(error: TransferError) -> Event {
    Event::Status {
        status: format!("error {}", error),
        detail: String::from("BLE proof transfer"),
    }
}

fn packet(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 4);
    packet.push(kind);
    packet.extend_from_slice(&[0, 0, 0]);
    packet.extend_from_slice(payload);
    packet
}
