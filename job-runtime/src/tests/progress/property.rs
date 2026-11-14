//! Random event sequences keep the FR14 job-sequence invariants.
use super::*;

/// A small deterministic generator so failures reproduce from the seed.
struct XorShift(u64);
impl XorShift {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

fn run_script(job: &Harness<Recorder>, random: &mut XorShift) -> TestResult {
    let mut completed = 0;
    for _ in 0..40 {
        match random.below(7) {
            0..=2 => {
                completed += random.below(5);
                let total = (random.below(2) == 0).then(|| completed + random.below(10));
                job.report(completed, total)?;
            }
            3 => {
                job.clock.advance(random.below(150));
                job.runtime.tick_progress()?;
            }
            4 => {
                job.runtime.pause(&job.id)?;
            }
            5 => {
                job.runtime.resume(&job.id)?;
            }
            _ if random.below(4) == 0 => {
                job.runtime.cancel(&job.id)?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[test]
fn random_sequences_keep_sequences_increasing_and_finish_once_and_last() -> TestResult {
    for seed in 1..=32_u64 {
        let mut random = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut job = recorded(ProgressUnit::Items)?;
        run_script(&job, &mut random)?;
        job.finish()?;
        let terminal = job.runtime.state(&job.id)?;
        assert!(terminal.is_terminal(), "seed {seed}");
        for _ in 0..4 {
            job.late_report(random.below(1_000))?;
            job.runtime.cancel(&job.id)?;
            job.runtime.pause(&job.id)?;
            job.next_batch()?;
        }
        assert_eq!(job.runtime.state(&job.id)?, terminal, "seed {seed}");
        let seen = job.sink.seen();
        check_order(&seen).map_err(|error| format!("seed {seed}: {error}"))?;
        assert_eq!(
            seen.last().map(Seen::sequence),
            Some(job.runtime.snapshot(&job.id)?.sequence),
            "seed {seed}"
        );
    }
    Ok(())
}
