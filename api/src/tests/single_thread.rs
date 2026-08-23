//! An opt-in guard for test suites that cannot run in parallel.
//!
//! The framework harness isolates tests by *transaction*, not by data: the
//! schema is created once per process and each test runs inside a transaction
//! that is rolled back on drop. Writes therefore stay invisible to other
//! tests — but the rows are still **locked** until the rollback.
//!
//! That is fine for a suite whose tests only touch their own rows, which is why
//! the api suite runs happily in parallel and must keep doing so. It is not
//! fine for a suite with table-wide statements: an unfiltered `UPDATE`, a
//! `TRUNCATE`, or a retention sweep has to lock rows that other tests have
//! inserted and not yet rolled back, so the two block on each other and
//! Postgres reports `deadlock detected` — intermittently, in a different test
//! each run.
//!
//! Such a suite pins itself with `RUST_TEST_THREADS = "1"` in its crate's
//! `.cargo/config.toml`. Cargo finds that file by walking up from the *working
//! directory*, so it applies to `cd <crate> && cargo test` and silently does
//! not apply to `cargo test --workspace` from the repo root. This guard is what
//! turns that silent gap into a named failure.

/// Where the effective thread count came from. Reported in the panic message,
/// because "you passed the wrong flag" and "you ran from the wrong directory"
/// need different fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadSource {
    /// libtest's `--test-threads` argument.
    Flag,
    /// The `RUST_TEST_THREADS` environment variable.
    Env,
    /// Neither was set, so libtest defaults to the machine's parallelism.
    Default,
}

/// Resolve the thread count libtest will actually use.
///
/// Pure, so it can be asserted without spawning a process — the same split as
/// `prometheus::render_config` in the CLI.
///
/// `args` is the test binary's argv, `env` the value of `RUST_TEST_THREADS`,
/// and `available` the machine's parallelism. libtest accepts both
/// `--test-threads N` and `--test-threads=N`, and the flag wins over the
/// environment variable.
#[must_use]
pub fn resolve_test_threads(
    args: &[String],
    env: Option<&str>,
    available: usize,
) -> (usize, ThreadSource) {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(value) = arg.strip_prefix("--test-threads=") {
            if let Ok(n) = value.parse() {
                return (n, ThreadSource::Flag);
            }
        } else if arg == "--test-threads" {
            if let Some(n) = it.next().and_then(|v| v.parse().ok()) {
                return (n, ThreadSource::Flag);
            }
        }
    }

    if let Some(n) = env.and_then(|v| v.trim().parse().ok()) {
        return (n, ThreadSource::Env);
    }

    (available, ThreadSource::Default)
}

/// Panic unless this test binary is running on a single thread.
///
/// Call it from the suite's own setup helper. It is deliberately **opt-in**:
/// [`super::setup_test`] never calls it, so parallel-safe suites are unaffected.
///
/// # Panics
///
/// When more than one test thread is in use, with a message naming the cause
/// and every way to fix it.
pub fn require_single_test_thread(suite: &str) {
    let args: Vec<String> = std::env::args().collect();
    let env = std::env::var("RUST_TEST_THREADS").ok();
    let available = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);

    let (threads, source) = resolve_test_threads(&args, env.as_deref(), available);
    if threads <= 1 {
        return;
    }

    let origin = match source {
        ThreadSource::Flag => "--test-threads",
        ThreadSource::Env => "RUST_TEST_THREADS",
        ThreadSource::Default => "libtest default — this machine's core count",
    };

    panic!(
        "\n\
         the {suite} test suite must run single-threaded.\n\n\
         \x20 detected  {threads} test threads ({origin})\n\n\
         Its tests share one schema and are isolated by a per-test transaction,\n\
         but several are table-wide by nature: the retention sweep deletes across\n\
         the whole table, and the regression tests use `UPDATE ... ` with no WHERE\n\
         and `TRUNCATE ... CASCADE`. Run in parallel they block on rows other\n\
         tests have inserted but not yet rolled back, and Postgres reports\n\
         `deadlock detected` — intermittently, in a different test each time.\n\n\
         Run it from the crate directory, where .cargo/config.toml sets\n\
         RUST_TEST_THREADS=1:\n\n\
         \x20   ./build.sh test\n\
         \x20   cd monitoring && cargo test\n\n\
         or pass the flag explicitly:\n\n\
         \x20   cargo test -p {suite} -- --test-threads=1\n\n\
         `cargo test --workspace` from the repo root cannot work: cargo has no\n\
         per-package test-threads setting, and serialising the whole workspace\n\
         would serialise the parallel-safe api suite too.\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn the_flag_wins_over_the_environment() {
        let a = args(&["test-bin", "--test-threads", "1"]);
        assert_eq!(
            resolve_test_threads(&a, Some("8"), 16),
            (1, ThreadSource::Flag)
        );
    }

    #[test]
    fn the_flag_is_accepted_in_both_spellings() {
        assert_eq!(
            resolve_test_threads(&args(&["b", "--test-threads=4"]), None, 16).0,
            4
        );
        assert_eq!(
            resolve_test_threads(&args(&["b", "--test-threads", "4"]), None, 16).0,
            4
        );
    }

    #[test]
    fn the_environment_is_used_when_no_flag_is_given() {
        assert_eq!(
            resolve_test_threads(&args(&["b"]), Some("1"), 16),
            (1, ThreadSource::Env)
        );
    }

    #[test]
    fn parallelism_is_the_fallback_and_is_reported_as_the_default() {
        assert_eq!(
            resolve_test_threads(&args(&["b"]), None, 16),
            (16, ThreadSource::Default)
        );
    }

    #[test]
    fn a_malformed_value_falls_through_rather_than_passing_the_guard() {
        // The dangerous failure would be parsing garbage as 1 and letting a
        // parallel run through, so garbage must fall back to the real count.
        assert_eq!(
            resolve_test_threads(&args(&["b", "--test-threads", "nonsense"]), None, 16),
            (16, ThreadSource::Default)
        );
        assert_eq!(resolve_test_threads(&args(&["b"]), Some(""), 16).0, 16);
    }

    #[test]
    fn a_single_core_machine_passes_honestly() {
        assert_eq!(
            resolve_test_threads(&args(&["b"]), None, 1),
            (1, ThreadSource::Default)
        );
    }
}
