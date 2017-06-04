use slog;
use slog::{Drain, Logger};
use slog_async;
use slog_term;
use std::fs;

type SimpleDrain = Drain<Ok = (), Err = slog::Never>;
type BoxedSimpleDrain = Drain<Ok = (), Err = ()>;

/// Set up logging
pub fn setup() -> Logger {
    // Log to a file
    let drain_file = {
        let log_path = "bittles.log";
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log_path)
            .unwrap();

        let decorator = slog_term::PlainSyncDecorator::new(file);
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        drain
    };

    // Log to the terminal
    let drain_term = {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::CompactFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        drain
    };

    let drain = slog::Duplicate::new(drain_file, drain_term).fuse();
    let log = slog::Logger::root(drain, o!());
    log
}
