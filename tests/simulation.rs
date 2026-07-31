//! An end-to-end simulation, written to be run under Miri.
//!
//! Before this file existed, `cargo +nightly miri test` exercised no simulation at all:
//! the only test in the tree read source files. The Miri gate named in
//! `specs/tech-stack.md` was therefore gating nothing, and the two undefined-behaviour
//! sites closed in Phase 2 could only be checked by hand with `miri run` over
//! `src/main.rs`.
//!
//! The model is deliberately shaped to touch the paths where `unsafe` lives:
//!
//! - a source with `Output = &mut f64` and a time event, so the solver's step loop and
//!   the aliasing between one system's output buffer and another's input buffer are
//!   exercised — this is the pair that produced the retag error;
//! - a **two-input** block, so `SystemDataIn for (&T, &U)` casts two leaves from two
//!   separate producers' buffers;
//! - a **held discrete** block, so `HeldSystem::update` runs — the site that used to
//!   duplicate a `&mut` through `ptr::read`;
//! - a recorder with `Output = ()`, so the zero-size buffer path is covered too.
//!
//! Per `specs/tech-stack.md`, the run is short under Miri and full length otherwise.

use std::cell::RefCell;

use ctrlsys::{
    pool::SystemPool,
    system::{
        System,
        discrete::{DiscreteSystem, holder::ZeroOrderHold},
    },
};

#[cfg(miri)]
const TOTAL_TIME: f64 = 0.6;
#[cfg(not(miri))]
const TOTAL_TIME: f64 = 10.0;

const MAX_STEP: f64 = 0.02;
const STEP_TIME: f64 = 0.15;
const SAMPLE_DT: f64 = 0.05;

thread_local! {
    static TRACE: RefCell<Vec<(usize, f64, f64)>> = const { RefCell::new(Vec::new()) };
}

/// Records everything it is fed. Stands in for the `Recorder` that Phase 5 introduces;
/// until then a test-local block is the only way to observe a signal.
///
/// Returns `f64::INFINITY`, so it contributes nothing to the solver's `next_time`
/// minimum and cannot change the step sequence of the model it observes.
struct Probe(usize);

impl<'s> System<'s> for Probe {
    type Input<'a> = &'a f64;
    type Output<'a> = ();

    fn update(&mut self, time: f64, input: Self::Input<'s>, _output: ()) -> f64 {
        let tag = self.0;
        let value = *input;
        TRACE.with(|t| t.borrow_mut().push((tag, time, value)));
        f64::INFINITY
    }
}

/// Zero until `STEP_TIME`, one afterwards. Requests a time event at the transition.
struct Step;

impl<'s> System<'s> for Step {
    type Input<'a> = ();
    type Output<'a> = &'a mut f64;

    fn update(&mut self, time: f64, (): (), output: Self::Output<'s>) -> f64 {
        if time >= STEP_TIME {
            *output = 1.0;
            f64::INFINITY
        } else {
            *output = 0.0;
            STEP_TIME
        }
    }
}

/// A first-order lag. Rate-dependent, deliberately: making it rate-independent is the
/// golden-test phase's job, and this test asserts the current behaviour so that phase's
/// change is visible rather than silent.
struct Lag;

impl<'s> System<'s> for Lag {
    type Input<'a> = &'a f64;
    type Output<'a> = &'a mut f64;

    fn update(&mut self, _time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64 {
        *output = *output * 0.95 + input * 0.05;
        f64::INFINITY
    }
}

/// Two inputs, from two different producers — the case that casts two leaves from two
/// separate buffers.
struct Difference;

impl<'s> System<'s> for Difference {
    type Input<'a> = (&'a f64, &'a f64);
    type Output<'a> = &'a mut f64;

    fn update(&mut self, _time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64 {
        *output = input.0 - input.1;
        f64::INFINITY
    }
}

/// Accumulates its input at a fixed rate. Wrapped in a `ZeroOrderHold`, so it runs
/// through `HeldSystem::update`.
struct Accumulator {
    total: f64,
}

impl DiscreteSystem for Accumulator {
    type Input<'a> = &'a f64;
    type Output<'a> = &'a mut f64;

    fn calculate(&mut self, _time: f64, input: Self::Input<'_>, output: &mut Self::Output<'_>) {
        self.total += input;
        **output = self.total;
    }

    fn timestep(&self) -> f64 {
        SAMPLE_DT
    }
}

fn run() -> Vec<(usize, f64, f64)> {
    TRACE.with(|t| t.borrow_mut().clear());

    let mut pool = SystemPool::new();

    let lag = pool.add_system(Lag);
    let step = pool.add_system(Step);
    pool.link(step, &lag);

    let difference = pool.add_system(Difference);
    pool.link((&step, &lag), &difference);

    let accumulator = pool.add_system(Accumulator { total: 0.0 }.with_holder(ZeroOrderHold::new()));
    pool.link(difference, &accumulator);

    let probe_step = pool.add_system(Probe(0));
    pool.link(step, &probe_step);
    let probe_difference = pool.add_system(Probe(1));
    pool.link(difference, &probe_difference);
    let probe_accumulator = pool.add_system(Probe(2));
    pool.link(accumulator, &probe_accumulator);

    pool.simulate(TOTAL_TIME, MAX_STEP)
        .expect("model has no algebraic loop");

    TRACE.with(|t| t.borrow().clone())
}

fn samples(trace: &[(usize, f64, f64)], tag: usize) -> Vec<(f64, f64)> {
    trace
        .iter()
        .filter(|(t, _, _)| *t == tag)
        .map(|(_, time, value)| (*time, *value))
        .collect()
}

#[test]
fn model_runs_and_every_block_is_observed() {
    let trace = run();

    for tag in 0..3 {
        assert!(
            !samples(&trace, tag).is_empty(),
            "probe {tag} recorded nothing; the model did not run as expected"
        );
    }

    let steps = samples(&trace, 0).len();
    assert!(
        steps >= (TOTAL_TIME / MAX_STEP) as usize,
        "expected at least one sample per step, got {steps}"
    );
}

#[test]
fn step_source_transitions_exactly_at_its_event() {
    for (time, value) in samples(&run(), 0) {
        let expected = if time >= STEP_TIME { 1.0 } else { 0.0 };
        assert!(
            (value - expected).abs() < f64::EPSILON,
            "step source was {value} at t={time}, expected {expected}"
        );
    }
}

#[test]
fn held_block_holds_between_samples() {
    let accumulated = samples(&run(), 2);

    // A zero-order-held accumulator is a staircase: never decreasing, and changing
    // value strictly fewer times than it is sampled.
    let mut changes = 0;
    for pair in accumulated.windows(2) {
        let (_, previous) = pair[0];
        let (_, current) = pair[1];
        assert!(
            current >= previous - f64::EPSILON,
            "accumulator decreased from {previous} to {current}"
        );
        if (current - previous).abs() > f64::EPSILON {
            changes += 1;
        }
    }

    assert!(
        changes < accumulated.len(),
        "output changed on every step; it is not being held between samples"
    );
    assert!(
        changes > 0,
        "output never changed; the discrete block never fired"
    );
}
