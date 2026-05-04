//! Parallel execution framework for multi-repo workspace ops.
//!
//! Multi-repo write-side commands — `ws clone`, `ws switch`, soon
//! `ws restore --all`, and the granular-scope `ws branch` — fan
//! out the same shape: pre-create one
//! [`indicatif::ProgressBar`] per affected child, spawn one
//! [`std::thread`] per child, run per-child work concurrently, and
//! collect results in declaration order. This module hosts that
//! shape so each consumer only has to express its per-item work
//! (a closure that takes an item and a bar) instead of re-deriving
//! the threading + bar-management boilerplate.
//!
//! Partial failures are accepted (Invariant 5: Partial Failure is
//! Acceptable). If some work closures return `Err`, the others
//! still run and their results are collected. The caller branches
//! on each `Result` to map success/failure into its own output
//! shape (`ChildResult` for clone, `ChildSwitchSummary` for
//! switch, etc.).
//!
//! The framework owns:
//!   - the [`MultiProgress`] container and the per-item
//!     [`ProgressBar`]s;
//!   - thread spawning via [`std::thread::scope`];
//!   - result collection in declaration order;
//!   - the final `multi.clear()` so subsequent stdout/stderr
//!     writes are not interleaved with bar leftovers.
//!
//! The work closure owns the bar's *lifecycle*: it must drive the
//! bar with `set_position`, `set_message`, etc. and call
//! `finish_with_message` to mark the bar terminal. The framework
//! only seeds an initial `"queued"` message on each bar.

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Run `work(item, bar)` in parallel for every item. Returns
/// results in declaration order: `out[i]` is the result for
/// `items[i]`. Empty input returns an empty `Vec` without spawning
/// threads or creating any bars.
///
/// Bounds:
///   - `R: Sync` — each thread borrows its item by reference.
///   - `T: Send` — each thread sends its result back across the
///     scope boundary.
///   - `F: Fn + Sync` — one closure shared across N threads.
pub fn execute<R, T, F>(items: &[R], label_fn: impl Fn(&R) -> String, work: F) -> Vec<Result<T>>
where
    R: Sync,
    T: Send,
    F: Fn(&R, &ProgressBar) -> Result<T> + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }

    let multi = MultiProgress::new();
    let style = bar_style();

    let labels: Vec<String> = items.iter().map(&label_fn).collect();
    let prefix_width = labels.iter().map(|l| l.len()).max().unwrap_or(0).max(8);
    let bars: Vec<ProgressBar> = labels
        .iter()
        .map(|label| {
            let bar = multi.add(ProgressBar::new(0));
            bar.set_style(style.clone());
            bar.set_prefix(format!("{label:<prefix_width$}"));
            bar.set_message("queued");
            bar
        })
        .collect();

    let work_ref = &work;
    let bars_ref = &bars;
    let items_ref = items;
    let results: Vec<Result<T>> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(items_ref.len());
        for i in 0..items_ref.len() {
            handles.push(scope.spawn(move || work_ref(&items_ref[i], &bars_ref[i])));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let _ = multi.clear();
    results
}

/// The shared progress-bar style for parallel workspace ops.
/// Docker-style: spinner + name + bar + counts + free-form
/// trailing message. Returned anew per call so each consumer can
/// hand it off to its bars without lifetime gymnastics.
pub fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {prefix} [{bar:30.cyan/blue}] {pos}/{len} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> ")
}

/// Render a duration in milliseconds as a short human string.
/// Switches from `ms` to `s` at the one-second boundary so bars
/// don't read "12345ms" for ops that take a few seconds.
pub fn format_ms(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", (ms as f64) / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn empty_input_returns_empty_vec() {
        let results: Vec<Result<()>> = execute::<i32, _, _>(&[], |i| i.to_string(), |_, _| Ok(()));
        assert!(results.is_empty());
    }

    #[test]
    fn single_item_runs_and_returns_result() {
        let items = [42];
        let results = execute(&items, |i| i.to_string(), |i, _bar| Ok(i * 2));
        assert_eq!(results.len(), 1);
        assert_eq!(*results[0].as_ref().unwrap(), 84);
    }

    #[test]
    fn results_preserve_declaration_order() {
        // Even if threads finish out of order, the returned Vec
        // matches items[i] -> out[i]. Use a delay inversely
        // proportional to the index so later items finish first.
        let items: Vec<usize> = (0..6).collect();
        let results = execute(
            &items,
            |i| format!("item-{i}"),
            |i, _bar| {
                let delay_ms = (10 - *i as u64) * 5;
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                Ok(*i * 100)
            },
        );
        assert_eq!(results.len(), 6);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(*r.as_ref().unwrap(), i * 100);
        }
    }

    #[test]
    fn partial_failures_are_collected_not_aborted() {
        let items = [1, 2, 3, 4];
        let results = execute(
            &items,
            |i| i.to_string(),
            |i, _bar| {
                if i % 2 == 0 {
                    Err(anyhow!("even number: {i}"))
                } else {
                    Ok(*i)
                }
            },
        );
        assert_eq!(results.len(), 4);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
        assert!(results[3].is_err());
        assert_eq!(*results[0].as_ref().unwrap(), 1);
        assert_eq!(*results[2].as_ref().unwrap(), 3);
    }

    #[test]
    fn work_closure_runs_concurrently() {
        // If everything ran sequentially, the max observed in-
        // flight count would be 1. With real concurrency, the max
        // should equal the number of items.
        let items: Vec<usize> = (0..4).collect();
        let in_flight = AtomicUsize::new(0);
        let max_in_flight = AtomicUsize::new(0);
        execute(
            &items,
            |i| format!("item-{i}"),
            |_, _bar| {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(40));
                in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok::<(), anyhow::Error>(())
            },
        );
        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "expected concurrent execution; max_in_flight={}",
            max_in_flight.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn format_ms_switches_unit_at_one_second() {
        assert_eq!(format_ms(0), "0ms");
        assert_eq!(format_ms(999), "999ms");
        assert_eq!(format_ms(1000), "1.0s");
        assert_eq!(format_ms(2500), "2.5s");
    }
}
