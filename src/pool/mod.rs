pub(crate) mod buffer;
pub(crate) mod graph;
pub mod link;

use std::{borrow::Cow, collections::HashSet, hash::Hash, marker::PhantomData};

use zerocopy::{FromBytes, Immutable, KnownLayout};

use crate::{
    pool::{
        buffer::AlignedBuffer,
        graph::{OrderError, simulation_order},
        link::{SystemIn, SystemLink, SystemOut, SystemRef},
    },
    system::{RawSystem, System},
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
    outputs: Vec<Vec<u8>>,
}

impl SystemPool {
    #[must_use]
    pub fn new() -> Self {
        Self {
            systems: Vec::new(),
            links: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[inline]
    #[must_use]
    pub fn get_output<In, Out>(&self, id: &SystemRef<In, Out>) -> &Out
    where
        Out: FromBytes + KnownLayout + Immutable,
    {
        FromBytes::ref_from_bytes(&self.outputs[id.id[0]]).expect("Indicated type was incorrect")
    }

    pub fn simulate(&mut self, total_time: f64, max_timestep: f64) -> Result<(), OrderError> {
        let order = simulation_order(self)?;

        let mut links_to_node = vec![vec![]; self.systems.len()];
        for link in &self.links {
            links_to_node[link.to_system_idx].push(link.clone());
        }

        // Pre-allocate buffers
        let mut input_buffers: Vec<AlignedBuffer> = (0..self.systems.len())
            .map(|idx| {
                let links = &links_to_node[idx];

                // Only allocate if we have mode than one input link
                if links.len() > 1 {
                    let input_size = links
                        .iter()
                        .map(|l| l.to_input_offset + l.num_bytes)
                        .max()
                        .unwrap_or(0);

                    let required_align = self.systems[idx].input_alignment();

                    AlignedBuffer::new(input_size, required_align)
                } else {
                    AlignedBuffer::new(0, 1)
                }
            })
            .collect();

        let mut time = 0.0;
        while time < total_time {
            let mut next_time = f64::INFINITY;

            let outputs_ptr = self.outputs.as_mut_ptr();

            for &idx in &order {
                let links = &links_to_node[idx];

                let input_cow: Cow<[u8]>;

                if links.len() == 1 {
                    // zero-copy path
                    let link = &links[0];
                    assert_eq!(link.to_input_offset, 0, "Single link must have zero offset");

                    // let output_slice = &self.outputs[link.from_system_idx];

                    // SAFETY: We know link.from_system_idx != idx
                    // because of the assertion in `SystemPool::link`.
                    // Therefore, this immutable borrow does not alias
                    // the mutable borrow we will create for output_buffer.
                    let output_slice = unsafe { &*(outputs_ptr.add(link.from_system_idx)) };

                    input_cow = Cow::Borrowed(output_slice);
                } else if links.len() > 1 {
                    // copy path
                    let input_buffer = &mut input_buffers[idx];
                    for link in links {
                        let output_slice = &self.outputs[link.from_system_idx];
                        let (offset, len) = (link.to_input_offset, link.num_bytes);
                        input_buffer[offset..offset + len].copy_from_slice(output_slice);
                    }

                    input_cow = Cow::Borrowed(input_buffer);
                } else {
                    // no inputs
                    input_cow = Cow::Borrowed(&[]);
                }

                let output_buffer = &mut self.outputs[idx];

                let nt = self.systems[idx].raw_update(time, input_cow, output_buffer);
                next_time = next_time.min(nt);
            }

            time = next_time.min(time + max_timestep);
        }

        Ok(())
    }

    pub fn add_system<'a, Sys, In, Out>(&mut self, sys: Sys) -> SystemRef<In, Out>
    where
        Sys: System<Input = In, Output = Out> + RawSystem + 'static,
        // Out: FromBytes,
    {
        self.systems.push(Box::new(sys));
        self.outputs.push(vec![0u8; size_of::<Out>()]);

        SystemRef {
            id: [self.systems.len() - 1],
            _io: PhantomData,
        }
    }

    pub fn link<SO, SI>(&mut self, from: &SO, to: &SI)
    where
        SO: SystemOut,
        SI: SystemIn<In = SO::Out>,
    {
        assert!(has_unique_elements(from.ids()));
        assert!(!from.ids().contains(&to.id()));
        from.add_links_to(to.id(), 0, &mut self.links)
    }
}
