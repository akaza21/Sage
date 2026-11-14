//! Cooperative WSL process ownership for the admitted local Codex CLI.
//!
//! Windows retains the production Job-object tree collector. This bounded
//! beta path deliberately owns only the direct app-server process, and is
//! represented as `TrustedCooperativeTree` in the launch contract.

use std::{
    io::{Read, Write},
    process::{ChildStdin, ChildStdout},
};

use sage_process::error::LaunchError;

use crate::acp::transport::{
    Frame, FrameError, FrameWriteOutcome, IncrementalFrameDecoder, OutboundFrame,
};

mod containment;
mod reap;
mod spawn;

pub(crate) use containment::TreeControl;
pub(crate) use reap::LaunchRecovery;
pub(crate) use spawn::launch;

pub(crate) struct StdinEndpoint(Option<ChildStdin>);

impl StdinEndpoint {
    pub(crate) fn write_frame(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .as_mut()
            .map_or(FrameWriteOutcome::ZeroByteFailure, |stdin| {
                let payload = frame.payload();
                match stdin
                    .write_all(payload)
                    .and_then(|()| stdin.write_all(b"\n"))
                    .and_then(|()| stdin.flush())
                {
                    Ok(()) => FrameWriteOutcome::Complete,
                    Err(_) if payload.is_empty() => FrameWriteOutcome::ZeroByteFailure,
                    Err(_) => FrameWriteOutcome::DeliveryUnknown { bytes_written: 0 },
                }
            })
    }

    pub(crate) fn close(&mut self) {
        self.0.take();
    }
}

pub(crate) struct StdoutEndpoint {
    stdout: ChildStdout,
    decoder: IncrementalFrameDecoder,
}

impl StdoutEndpoint {
    pub(crate) fn read(&mut self, target: &mut [u8]) -> Result<usize, LaunchError> {
        if target.len() as u64 > sage_core::limits::LIMITS.wire.frame_bytes {
            return Err(LaunchError::StdioLimit);
        }
        self.stdout.read(target).map_err(|_| LaunchError::Io)
    }

    pub(crate) fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            if let Some(frame) = self.decoder.drain_pending()? {
                return Ok(Some(frame));
            }
            let read = self.stdout.read(&mut chunk).map_err(|_| FrameError::Io)?;
            if read == 0 {
                return self.decoder.finish();
            }
            if let Some(frame) = self.decoder.push_chunk(&chunk[..read])? {
                return Ok(Some(frame));
            }
        }
    }
}

pub(crate) struct RunningTree {
    stdin: StdinEndpoint,
    stdout: StdoutEndpoint,
    control: TreeControl,
}

impl RunningTree {
    fn new(stdin: ChildStdin, stdout: ChildStdout, control: TreeControl) -> Self {
        Self {
            stdin: StdinEndpoint(Some(stdin)),
            stdout: StdoutEndpoint {
                stdout,
                decoder: IncrementalFrameDecoder::new(),
            },
            control,
        }
    }

    pub(crate) fn into_parts(self) -> (StdinEndpoint, StdoutEndpoint, TreeControl) {
        (self.stdin, self.stdout, self.control)
    }
}

#[cfg(test)]
pub(crate) fn launch_fixture_for_test(runtime_epoch: u64) -> RunningTree {
    use std::{ffi::OsString, fs::File, io::Read};

    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    let executable = std::fs::canonicalize("/bin/sh").unwrap();
    let mut file = File::open(&executable).unwrap();
    let byte_len = file.metadata().unwrap().len();
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    let project_root = crate::project_registry::ProjectRootAuthority::for_test(
        crate::acp::contract::SageProjectId::parse("project-process-fixture").unwrap(),
        crate::acp::contract::ProjectGeneration::ZERO,
        crate::acp::contract::SageRootId::parse("root-process-fixture").unwrap(),
        &std::env::current_dir().unwrap(),
    )
    .unwrap();
    let spec = super::LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch,
        artifact: super::ArtifactExpectation::new(
            Uuid::nil(),
            executable,
            digest.finalize().into(),
            byte_len,
        )
        .unwrap(),
        fixed_args: super::BoundedOsArgs::new(vec![
            OsString::from("-c"),
            OsString::from("exec sleep 30"),
        ])
        .unwrap(),
        environment: super::BoundedEnvironment::new(Vec::new()).unwrap(),
        project_root,
        required_guarantee: super::RequiredContainment::TrustedCooperativeTree,
        limits: super::TreeLimits::ade_0006a(),
        deadlines: super::ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
    };
    let pool = super::TreeCapacityPool::isolated_ade_0006a();
    launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap()).unwrap()
}

#[cfg(test)]
mod tests;
