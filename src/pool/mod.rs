pub(crate) mod buffer;
pub(crate) mod graph;
pub mod link;

use std::{collections::HashSet, hash::Hash, marker::PhantomData};

use crate::{
    pool::{
        buffer::BufferSet,
        graph::{OrderError, simulation_order},
        link::{SystemIn, SystemLink, SystemMux, SystemOut, SystemRef},
    },
    system::{RawSystem, System, SystemDataIn, SystemDataOut},
};

fn has_unique_elements<T>(iter: T) -> bool
where
    T: IntoIterator,
    T::Item: Eq + Hash,
{
    let mut uniq = HashSet::new();
    iter.into_iter().all(move |x| uniq.insert(x))
}

pub struct SystemPool {
    systems: Vec<Box<dyn RawSystem>>,
    links: Vec<SystemLink>,
}

impl Default for SystemPool {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Run the model from `t = 0` to `total_time`.
    ///
    /// The execution order is computed once, up front. Each step every block is
    /// updated in that order and returns the next absolute time it wants to be called
    /// at; the step advances to the earliest such time, capped by `max_timestep`.
    ///
    /// # Errors
    ///
    /// Returns [`OrderError`] if no execution order exists — that is, if the graph
    /// contains an algebraic loop: a cycle in which no block breaks direct
    /// feedthrough. Nothing is simulated in that case; the error is raised during
    /// setup, before any block is updated.
    ///
    /// # Panics
    ///
    /// Panics if a link names a system index the pool does not contain, which cannot
    /// happen through the public API: indices are handed out by `add_system`.
    ///
    /// An empty pool no longer panics. It used to, on a `max().unwrap()` over the
    /// per-system link counts; that reduction disappeared when the input slices moved
    /// behind `BufferSet::with_split`, which takes the indices for one system at a
    /// time and never needs the maximum arity. `specs/roadmap.md`'s
    /// solver-hardening item for zero systems is therefore already satisfied here.
    pub fn simulate(&mut self, total_time: f64, max_timestep: f64) -> Result<(), OrderError> {
        let order = simulation_order(self)?;

        let mut links_to_node = vec![vec![]; self.systems.len()];
        for link in &self.links {
            links_to_node[link.to_system_idx].push(link.clone());
        }

        // Pre-allocate buffers. `AlignedBuffer::new(0, ..)` handles a zero-size output
        // without allocating.
        let mut output_buffers = BufferSet::new(
            self.systems
                .iter()
                .map(|sys| (sys.output_size(), sys.output_alignment())),
        );

        // Reused across steps so the per-system source list is not rebuilt each time.
        let mut input_idx_buffer: Vec<usize> = Vec::new();

        let mut time = 0.0;
        while time < total_time {
            let mut next_time = f64::INFINITY;

            for &idx in &order {
                let links = &links_to_node[idx];

                input_idx_buffer.clear();
                input_idx_buffer.extend(links.iter().map(|link| link.from_system_idx));

                // The disjoint borrow lives in `pool::buffer`: this system's output is
                // borrowed uniquely while its producers' outputs are borrowed shared.
                // `with_split` asserts the disjointness that makes it sound, so there is
                // no `unsafe` in the solver — see `specs/mission.md` Article 1.
                let system = &mut self.systems[idx];
                let nt = output_buffers.with_split(
                    idx,
                    &input_idx_buffer,
                    |output_slice, input_slices| {
                        let mut output_ref_buffer = [output_slice]; // TODO extend for mux outputs
                        system.raw_update(time, input_slices, &mut output_ref_buffer)
                    },
                );
                next_time = next_time.min(nt);
            }

            // eprintln!("Next: {}", next_time);
            time = next_time.min(time + max_timestep);
        }

        Ok(())
    }

    pub fn add_system<'a, Sys, In, Out>(&mut self, sys: Sys) -> SystemRef<In::Payload, Out::Payload>
    where
        Sys: System<'a, Input<'a> = In, Output<'a> = Out> + RawSystem + 'static,
        In: SystemDataIn<'a>,
        Out: SystemDataOut<'a>,
        In::Payload: Sized,
        Out::Payload: Sized,
    {
        self.systems.push(Box::new(sys));

        SystemRef {
            id: [self.systems.len() - 1],
            _io: PhantomData,
        }
    }

    /// Wire the outputs of `from` to the input of `to`.
    ///
    /// Type agreement is a compile-time property: the `SI: SystemIn<In = Out>` bound
    /// means a producer whose output type differs from the consumer's input type does
    /// not compile (`specs/mission.md` Article 2). What remains checked at run time is
    /// below.
    ///
    /// # Panics
    ///
    /// Panics if `from` names the same producer twice, or if `to` appears among the
    /// producers in `from` — a system feeding itself. Both are user contract
    /// violations, so per `specs/mission.md` Article 4 the checks are always on rather
    /// than `debug_assert!`.
    ///
    /// The second check is load-bearing for soundness, not merely for sanity: it is
    /// what keeps a system's own buffer out of its input list, which is the
    /// precondition `BufferSet::with_split` asserts before borrowing one buffer
    /// mutably and the rest shared. Violating it does not reach undefined behaviour —
    /// `with_split` panics — but it is what makes that panic unreachable.
    pub fn link<SI, Out>(&mut self, from: impl Into<SystemMux<Out>>, to: &SI)
    where
        SI: SystemIn<In = Out>,
    {
        let from: SystemMux<Out> = from.into();
        assert!(has_unique_elements(from.ids()));
        assert!(!from.ids().contains(&to.id()));
        from.add_links_to(to.id(), 0, &mut self.links);
    }
}
