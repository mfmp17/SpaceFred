use std::sync::mpsc::Sender;

use crate::app::{WorkerCommand, macos, status_item};

/// Which restoration the worker runs: a plain one, or one that also undoes the
/// setup tour's arrangement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum RestoreKind {
    Restore,
    /// Also puts back the frames the setup tour's arrangement changed.
    Undo,
}

impl RestoreKind {
    /// Two requests for one restoration: an undo includes a plain restore.
    const fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::Restore, Self::Restore) => Self::Restore,
            _ => Self::Undo,
        }
    }
}

/// What follows the ordinary command in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum AfterCommand {
    Nothing,
    /// A restoration was requested while the command ran.
    StartRestore(RestoreKind),
    /// The command was sent while a failed restoration waited for its retry.
    RetryRestore(RestoreKind),
}

/// What the main thread knows the worker is doing. Each variant is one legal
/// state; the five booleans this replaces (`busy`, `interruptible`,
/// `restore_pending`, `restoring`, `undo`) allowed 32 combinations, such as a
/// restoration running with no command in flight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum WorkerLifecycle {
    /// Nothing in flight and no restoration owed.
    Idle,
    /// An ordinary command is in flight.
    Running {
        interruptible: bool,
        then: AfterCommand,
    },
    /// A restoration is in flight; `again` is one requested while it runs.
    Restoring {
        kind: RestoreKind,
        again: Option<RestoreKind>,
    },
    /// The last restoration left windows parked; the app loop's timer retries it.
    RestoreRetryDue(RestoreKind),
    /// A restoration is owed and nothing is in flight: the next
    /// [`WorkerDispatch::start_pending_restore`] sends it.
    RestorePending(RestoreKind),
}

pub(in crate::app) struct WorkerDispatch {
    commands: Sender<WorkerCommand>,
    lifecycle: WorkerLifecycle,
    /// Terminal flag set once the worker channel disconnects or rejects a send;
    /// prevents resuming a dead worker into a silent zombie.
    gone: bool,
}

impl WorkerDispatch {
    pub(in crate::app) const fn new(commands: Sender<WorkerCommand>) -> Self {
        Self {
            commands,
            lifecycle: WorkerLifecycle::Idle,
            gone: false,
        }
    }

    #[cfg(test)]
    pub(in crate::app) const fn lifecycle(&self) -> WorkerLifecycle {
        self.lifecycle
    }

    /// True once the worker thread has disconnected; this state is terminal.
    pub(in crate::app) const fn is_gone(&self) -> bool {
        self.gone
    }

    pub(in crate::app) const fn is_busy(&self) -> bool {
        matches!(
            self.lifecycle,
            WorkerLifecycle::Running { .. } | WorkerLifecycle::Restoring { .. }
        )
    }

    pub(in crate::app) const fn is_interruptible_busy(&self) -> bool {
        matches!(
            self.lifecycle,
            WorkerLifecycle::Running {
                interruptible: true,
                ..
            }
        )
    }

    /// A restoration is running or waiting for its retry.
    pub(in crate::app) const fn is_restoring(&self) -> bool {
        matches!(
            self.lifecycle,
            WorkerLifecycle::Restoring { .. }
                | WorkerLifecycle::RestoreRetryDue(_)
                | WorkerLifecycle::Running {
                    then: AfterCommand::RetryRestore(_),
                    ..
                }
        )
    }

    /// A restoration is running, waiting for its retry, or owed.
    pub(in crate::app) const fn is_restoring_or_pending(&self) -> bool {
        !matches!(
            self.lifecycle,
            WorkerLifecycle::Idle
                | WorkerLifecycle::Running {
                    then: AfterCommand::Nothing,
                    ..
                }
        )
    }

    /// Sends an ordinary command. Callers send only while the worker is not
    /// busy; a restoration owed stays owed until the command finishes.
    pub(in crate::app) fn send(&mut self, command: WorkerCommand) -> bool {
        if self.gone {
            return false;
        }
        let interruptible = command.interruptible();
        if self.commands.send(command).is_err() {
            self.lifecycle = WorkerLifecycle::Idle;
            self.gone = true;
            return false;
        }
        self.lifecycle = match self.lifecycle {
            WorkerLifecycle::Idle => WorkerLifecycle::Running {
                interruptible,
                then: AfterCommand::Nothing,
            },
            WorkerLifecycle::Running { then, .. } => WorkerLifecycle::Running {
                interruptible,
                then,
            },
            restoring @ WorkerLifecycle::Restoring { .. } => restoring,
            WorkerLifecycle::RestoreRetryDue(kind) => WorkerLifecycle::Running {
                interruptible,
                then: AfterCommand::RetryRestore(kind),
            },
            WorkerLifecycle::RestorePending(kind) => WorkerLifecycle::Running {
                interruptible,
                then: AfterCommand::StartRestore(kind),
            },
        };
        true
    }

    pub(in crate::app) fn send_with_outputs(
        &mut self,
        command: impl FnOnce(macos::DisplayOutputs) -> WorkerCommand,
    ) -> bool {
        match macos::outputs() {
            Ok(outputs) => self.send(command(outputs)),
            Err(error) => {
                status_item::set_status(&format!("Display error: {error}"));
                false
            }
        }
    }

    /// Sends the restoration owed (a plain one when none is), returning whether
    /// the worker took it. A worker that cannot take it owes nothing more.
    pub(in crate::app) fn send_restore(&mut self) -> bool {
        let kind = match self.lifecycle {
            WorkerLifecycle::RestoreRetryDue(kind) | WorkerLifecycle::RestorePending(kind) => kind,
            WorkerLifecycle::Restoring { kind, again } => {
                again.map_or(kind, |again| kind.and(again))
            }
            WorkerLifecycle::Running {
                then: AfterCommand::StartRestore(kind) | AfterCommand::RetryRestore(kind),
                ..
            } => kind,
            WorkerLifecycle::Idle
            | WorkerLifecycle::Running {
                then: AfterCommand::Nothing,
                ..
            } => RestoreKind::Restore,
        };
        self.send_restore_of(kind)
    }

    fn send_restore_of(&mut self, kind: RestoreKind) -> bool {
        if self.gone {
            return false;
        }
        let outputs = macos::outputs().ok();
        let command = match kind {
            RestoreKind::Restore => WorkerCommand::RestoreAll(outputs),
            RestoreKind::Undo => WorkerCommand::UndoArrangement(outputs),
        };
        let sent = self.commands.send(command).is_ok();
        if sent {
            self.lifecycle = WorkerLifecycle::Restoring { kind, again: None };
        } else {
            self.lifecycle = WorkerLifecycle::Idle;
            self.gone = true;
        }
        sent
    }

    /// Restore every window now, or once the command in flight finishes.
    pub(in crate::app) fn request_restore(&mut self, kind: RestoreKind) {
        if self.gone {
            return;
        }
        match self.lifecycle {
            WorkerLifecycle::Running {
                interruptible,
                then,
            } => {
                let kind = match then {
                    AfterCommand::Nothing => kind,
                    AfterCommand::StartRestore(owed) | AfterCommand::RetryRestore(owed) => {
                        owed.and(kind)
                    }
                };
                self.lifecycle = WorkerLifecycle::Running {
                    interruptible,
                    then: AfterCommand::StartRestore(kind),
                };
            }
            WorkerLifecycle::Restoring {
                kind: running,
                again,
            } => {
                self.lifecycle = WorkerLifecycle::Restoring {
                    kind: running,
                    again: Some(again.map_or(kind, |again| again.and(kind))),
                };
            }
            WorkerLifecycle::Idle => {
                self.send_restore_of(kind);
            }
            WorkerLifecycle::RestoreRetryDue(owed) | WorkerLifecycle::RestorePending(owed) => {
                self.send_restore_of(owed.and(kind));
            }
        }
    }

    /// The ordinary command in flight finished.
    pub(in crate::app) fn finish_command(&mut self) {
        if let WorkerLifecycle::Running { then, .. } = self.lifecycle {
            self.lifecycle = match then {
                AfterCommand::Nothing => WorkerLifecycle::Idle,
                AfterCommand::StartRestore(kind) => WorkerLifecycle::RestorePending(kind),
                AfterCommand::RetryRestore(kind) => WorkerLifecycle::RestoreRetryDue(kind),
            };
        }
    }

    /// The restoration in flight put every window back (or the app loop gave
    /// up on it). A restoration requested meanwhile is still owed.
    pub(in crate::app) fn finish_restore(&mut self) {
        self.lifecycle = match self.lifecycle {
            WorkerLifecycle::Restoring {
                again: Some(kind), ..
            }
            | WorkerLifecycle::Running {
                then: AfterCommand::StartRestore(kind),
                ..
            }
            | WorkerLifecycle::RestorePending(kind) => WorkerLifecycle::RestorePending(kind),
            _ => WorkerLifecycle::Idle,
        };
    }

    /// The restoration in flight left windows parked; retry it on the timer.
    pub(in crate::app) fn defer_restore_retry(&mut self) {
        if self.gone {
            return;
        }
        self.lifecycle = match self.lifecycle {
            WorkerLifecycle::Restoring {
                kind,
                again: Some(again),
            } => WorkerLifecycle::RestorePending(kind.and(again)),
            WorkerLifecycle::Restoring { kind, again: None }
            | WorkerLifecycle::RestoreRetryDue(kind)
            | WorkerLifecycle::Running {
                then: AfterCommand::RetryRestore(kind),
                ..
            } => WorkerLifecycle::RestoreRetryDue(kind),
            WorkerLifecycle::RestorePending(kind)
            | WorkerLifecycle::Running {
                then: AfterCommand::StartRestore(kind),
                ..
            } => WorkerLifecycle::RestorePending(kind),
            WorkerLifecycle::Idle
            | WorkerLifecycle::Running {
                then: AfterCommand::Nothing,
                ..
            } => WorkerLifecycle::RestoreRetryDue(RestoreKind::Restore),
        };
    }

    /// Sends a restoration owed while nothing is in flight. `None` when there
    /// is none to send; otherwise whether the worker took it.
    pub(in crate::app) fn start_pending_restore(&mut self) -> Option<bool> {
        let WorkerLifecycle::RestorePending(kind) = self.lifecycle else {
            return None;
        };
        Some(self.send_restore_of(kind))
    }

    pub(in crate::app) fn reset_disconnected(&mut self) {
        self.lifecycle = WorkerLifecycle::Idle;
        self.gone = true;
    }

    pub(in crate::app) fn stop(&self) {
        drop(self.commands.send(WorkerCommand::Stop));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn disconnected_worker_enters_terminal_gone_state() {
        let (tx, rx) = mpsc::channel();
        let mut dispatch = WorkerDispatch::new(tx);
        assert!(!dispatch.is_gone());

        dispatch.reset_disconnected();
        assert!(dispatch.is_gone());
        assert!(!dispatch.is_busy());
        assert!(!dispatch.is_restoring());
        assert!(!dispatch.is_restoring_or_pending());

        // Even while the receiver still exists, a gone dispatch refuses all transitions.
        assert!(!dispatch.send(WorkerCommand::Stop));
        assert!(!dispatch.send_restore());
        dispatch.request_restore(RestoreKind::Restore);
        dispatch.defer_restore_retry();
        assert_eq!(dispatch.start_pending_restore(), None);
        assert!(dispatch.is_gone());
        assert!(!dispatch.is_busy());
        assert!(!dispatch.is_restoring_or_pending());
        assert!(rx.try_recv().is_err());

        // Dropping the receiver also transitions a fresh dispatch to gone on send failure.
        let (tx2, rx2) = mpsc::channel();
        drop(rx2);
        let mut dispatch2 = WorkerDispatch::new(tx2);
        assert!(!dispatch2.send(WorkerCommand::Stop));
        assert!(dispatch2.is_gone());
    }
}
