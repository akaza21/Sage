use crate::acp::process::{LaunchError, StdoutEndpoint};

use super::{DriverMailbox, ProcessTreeControl, ReaderTerminal};

const READ_CHUNK_BYTES: usize = 8 * 1024;

pub(crate) trait BlockingStdout {
    fn read_chunk(&mut self, target: &mut [u8]) -> Result<usize, LaunchError>;
}

impl BlockingStdout for StdoutEndpoint {
    fn read_chunk(&mut self, target: &mut [u8]) -> Result<usize, LaunchError> {
        self.read(target)
    }
}

pub(crate) struct BlockingStdoutReader<R> {
    stdout: R,
}

pub(crate) struct ProcessExitWatcher<C> {
    control: C,
}

impl<C: ProcessTreeControl> ProcessExitWatcher<C> {
    pub(crate) fn new(control: C) -> Self {
        Self { control }
    }

    /// Observes only the root handle. The serialized driver retains teardown
    /// authority and is the sole consumer of the linear zero-process proof.
    pub(crate) fn run(self, mailbox: &DriverMailbox) {
        self.control
            .log_wait_handle_observation("acp.process_exit_watcher.attached");
        match self.control.observe_root_exit() {
            Ok(exit) => {
                self.control
                    .log_wait_handle_observation("acp.process_exit_watcher.exited");
                tracing::warn!(
                    root_exit = ?exit,
                    sage_requested_termination = !matches!(exit, crate::acp::process::TreeExit::Exited { .. }),
                    "ACP child root exit observed"
                );
                let _ = mailbox.signal_process_exit(exit);
            }
            Err(_) => {
                let _ = mailbox.signal_reader_terminal(ReaderTerminal::Io);
            }
        }
    }
}

impl<R: BlockingStdout> BlockingStdoutReader<R> {
    pub(crate) fn new(stdout: R) -> Self {
        Self { stdout }
    }

    /// Runs the sole blocking native stdout reader. It never decodes ACP and
    /// never creates a task per frame; one bounded raw chunk is handed to the
    /// real async AdapterPump dispatcher at a time.
    pub(crate) fn run(mut self, mailbox: &DriverMailbox) {
        let mut target = [0_u8; READ_CHUNK_BYTES];
        loop {
            match self.stdout.read_chunk(&mut target) {
                Ok(0) => {
                    tracing::warn!("ACP stdout EOF observed");
                    let _ = mailbox.signal_reader_terminal(ReaderTerminal::Eof);
                    return;
                }
                Ok(read) => {
                    if read > target.len() {
                        let _ = mailbox.signal_reader_terminal(ReaderTerminal::Io);
                        return;
                    }
                    if mailbox
                        .push_chunk_blocking(target[..read].to_vec())
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => {
                    let _ = mailbox.signal_reader_terminal(ReaderTerminal::Io);
                    return;
                }
            }
        }
    }
}
