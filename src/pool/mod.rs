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

impl SystemPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            links: Vec::new(),
        }
    }

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

        let inp_max = links_to_node.iter().map(|x| x.len()).max().unwrap();

        let mut input_ref_buffer = vec![&[][..]; inp_max];
        let mut output_ref_buffer = vec![&mut [][..]]; // TODO change this when adding mux inputs

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

                let data_ptr = output_buf.as_ptr() as *mut u8;
                let len = output_buf.len();

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

    pub fn link<SI, Out>(&mut self, from: impl Into<SystemMux<Out>>, to: &SI)
    where
        SI: SystemIn<In = Out>,
    {
        let from: SystemMux<Out> = from.into();
        assert!(has_unique_elements(from.ids()));
        assert!(!from.ids().contains(&to.id()));
        from.add_links_to(to.id(), 0, &mut self.links)
    }
}
