pub(crate) mod buffer;
pub(crate) mod graph;
pub mod link;

use std::{collections::HashSet, hash::Hash, marker::PhantomData};

use crate::{
    pool::{
        buffer::AlignedBuffer,
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
    /// Panics if the pool contains no systems, because the per-system link counts are
    /// reduced with `max().unwrap()` over an empty iterator. Handling zero systems is
    /// a `specs/roadmap.md` Phase 3 item.
    ///
    /// Also panics if a link names a system index the pool does not contain, which
    /// cannot happen through the public API: indices are handed out by `add_system`.
    pub fn simulate(&mut self, total_time: f64, max_timestep: f64) -> Result<(), OrderError> {
        let order = simulation_order(self)?;

        let mut links_to_node = vec![vec![]; self.systems.len()];
        for link in &self.links {
            links_to_node[link.to_system_idx].push(link.clone());
        }

        // Pre-allocate buffers
        let output_buffers: Vec<AlignedBuffer> = self
            .systems
            .iter()
            .map(|sys| {
                let size = sys.output_size();
                let align = sys.output_alignment();

                // Note: AlignedBuffer::new(0, ...) handles 0-size correctly
                // if you implemented it to handle size 0 gracefullly.
                AlignedBuffer::new(size, align)
            })
            .collect();

        let inp_max = links_to_node.iter().map(Vec::len).max().unwrap();

        let mut input_ref_buffer = vec![&[][..]; inp_max];
        let mut output_ref_buffer = [&mut [][..]]; // TODO change this when adding mux inputs

        let mut time = 0.0;
        while time < total_time {
            let mut next_time = f64::INFINITY;

            for &idx in &order {
                let links = &links_to_node[idx];

                for (i, link) in links.iter().enumerate() {
                    input_ref_buffer[i] = &output_buffers[link.from_system_idx][..];
                }

                // Here happens the error. We know we can do this, since we have checks for inputs and outputs not to clash,
                // so the mutable and immutable references will never clash
                let output_buf = &output_buffers[idx];

                let data_ptr = output_buf.as_ptr().cast_mut();
                let len = output_buf.len();

                #[expect(
                    clippy::undocumented_unsafe_blocks,
                    reason = "KNOWN UNSOUND — Phase 2 fixes this. `output_buf` is a \
                              *shared* borrow, so casting its `as_ptr()` to `*mut u8` \
                              does not grant write permission and this call retags a \
                              SharedReadOnly tag as Unique. Miri confirms it. The \
                              aliasing argument in the comment above is correct in \
                              spirit and not established by this code. No true \
                              `// SAFETY:` comment exists, so none is written. See \
                              specs/roadmap.md Phase 2."
                )]
                let output_slice = unsafe { std::slice::from_raw_parts_mut(data_ptr, len) };

                output_ref_buffer[0] = output_slice;

                let nt = self.systems[idx].raw_update(
                    time,
                    &input_ref_buffer[..links.len()],
                    &mut output_ref_buffer[..1],
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
    /// what guarantees no buffer is borrowed shared (as an input) and unique (as an
    /// output) in the same step, which every `// SAFETY:` comment in
    /// `RawSystem::raw_update` cites.
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
