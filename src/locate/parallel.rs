//! Scoped-thread bands for the search passes. Results never depend on the
//! thread count: bands are contiguous and returned in order.
use crate::error::Result;
use std::sync::atomic::{AtomicBool, Ordering};

type Check<'a> = &'a mut dyn FnMut() -> Result<()>;

/// Worker threads for the search passes. Bounded: the
/// passes are memory-bound well before every core of a large machine is busy.
fn workers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
}

// Spawning costs tens of microseconds; small searches stay on the caller's thread.
const PARALLEL_MIN_WORK: usize = 1 << 16;

/// Run `job` over `items` split into contiguous bands on scoped threads,
/// returning each band's final state in band order. Items of one band are
/// visited in order. The calling thread works the first band and polls the
/// fail-safe `check` before each item, stopping the other bands on error.
pub(crate) fn parallel<I, S>(
    items: Vec<I>,
    work: usize,
    init: impl Fn() -> S + Sync,
    job: impl Fn(&mut S, I) + Sync,
    check: Check<'_>,
) -> Result<Vec<S>>
where
    I: Send,
    S: Send,
{
    let threads = workers()
        .min(work / PARALLEL_MIN_WORK)
        .min(items.len())
        .max(1);
    let band = items.len().div_ceil(threads).max(1);
    let mut bands = Vec::with_capacity(threads);
    let mut rest = items;
    while rest.len() > band {
        let tail = rest.split_off(band);
        bands.push(rest);
        rest = tail;
    }
    bands.push(rest);
    let abort = AtomicBool::new(false);
    let (init, job, abort) = (&init, &job, &abort);
    std::thread::scope(|scope| {
        let mut bands = bands.into_iter();
        let first = bands.next().unwrap_or_default();
        let handles: Vec<_> = bands
            .map(|band| {
                scope.spawn(move || {
                    let mut state = init();
                    for item in band {
                        if abort.load(Ordering::Relaxed) {
                            break;
                        }
                        job(&mut state, item);
                    }
                    state
                })
            })
            .collect();
        let mut state = init();
        let mut result = Ok(());
        for item in first {
            if let Err(error) = check() {
                abort.store(true, Ordering::Relaxed);
                result = Err(error);
                break;
            }
            job(&mut state, item);
        }
        let mut states = Vec::with_capacity(handles.len() + 1);
        states.push(state);
        for handle in handles {
            match handle.join() {
                Ok(state) => states.push(state),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
        result.map(|()| states)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::types::Point;

    #[test]
    fn bands_preserve_item_order_and_visit_everything() {
        for (items, work) in [
            (0usize, 0usize),
            (1, 1 << 24),
            (7, 1 << 24),
            (1000, 1 << 24),
            (1000, 10),
        ] {
            let mut checks = 0;
            let bands = parallel(
                (0..items).collect(),
                work,
                Vec::new,
                |seen: &mut Vec<usize>, item: usize| seen.push(item),
                &mut || {
                    checks += 1;
                    Ok(())
                },
            )
            .unwrap();
            let flat: Vec<usize> = bands.into_iter().flatten().collect();
            assert_eq!(flat, (0..items).collect::<Vec<_>>());
            assert!(checks >= items.min(1), "fail-safe never polled");
        }
    }

    #[test]
    fn fail_safe_error_stops_the_search() {
        let visited = std::sync::atomic::AtomicUsize::new(0);
        let mut checks = 0;
        let result = parallel(
            (0..10_000).collect(),
            1 << 24,
            || (),
            |_, _: usize| {
                visited.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_micros(50));
            },
            &mut || {
                checks += 1;
                if checks == 3 {
                    Err(Error::FailSafe(Point::new(0, 0)))
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(result, Err(Error::FailSafe(_))));
        assert_eq!(checks, 3);
        assert!(visited.load(Ordering::Relaxed) < 10_000);
    }
}
