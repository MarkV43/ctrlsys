pub mod graph;

use std::{borrow::Cow, collections::HashSet, f64, hash::Hash, marker::PhantomData};

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::system::graph::{OrderError, simulation_order};

fn has_unique_elements<T>(iter: T) -> bool
where
    T: IntoIterator,
    T::Item: Eq + Hash,
{
    let mut uniq = HashSet::new();
    iter.into_iter().all(move |x| uniq.insert(x))
}

#[derive(Debug, Clone)]
pub struct SystemLayout {
    offset: usize,
    len: usize,
}

// SAFETY: We need this type to be `packed`,
// since otherwise there'd be no way to reconstruct
// it from the two inner types' bytes.
#[derive(Debug, Clone, FromBytes, IntoBytes, Immutable, KnownLayout, PartialEq, Eq)]
#[repr(C)]
pub struct Pair<T, U>(pub T, pub U);

pub trait SystemIn<In> {
    fn id(&self) -> usize;
}
pub trait SystemOut<Out> {
    fn ids(&self) -> &[usize];
    fn layouts(&self) -> Vec<SystemLayout>;
    fn add_layouts(&self, buffer: &mut Vec<SystemLayout>, base_offset: usize);
    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>)
    where
        Out: KnownLayout;
}

pub struct SystemRef<In, Out> {
    id: [usize; 1],
    _io: PhantomData<(In, Out)>,
}

impl<In, Out> SystemIn<In> for SystemRef<In, Out> {
    fn id(&self) -> usize {
        self.id[0]
    }
}
impl<In, Out> SystemOut<Out> for SystemRef<In, Out> {
    fn ids(&self) -> &[usize] {
        &self.id
    }

    fn layouts(&self) -> Vec<SystemLayout> {
        vec![SystemLayout {
            offset: 0,
            len: size_of::<Out>(),
        }]
    }

    fn add_layouts(&self, buffer: &mut Vec<SystemLayout>, base_offset: usize) {
        buffer.push(SystemLayout {
            offset: base_offset,
            len: size_of::<Out>(),
        })
    }

    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>)
    where
        Out: KnownLayout,
    {
        links.push(SystemLink {
            from_system_idx: self.id[0],
            to_system_idx: to_id,
            to_input_offset: base_offset,
            num_bytes: size_of::<Out>(),
        })
    }
}

pub struct SystemMux<Out> {
    ids: Vec<usize>,
    layouts: Vec<SystemLayout>,
    _io: PhantomData<Out>,
}

impl<Out> SystemOut<Out> for SystemMux<Out> {
    fn ids(&self) -> &[usize] {
        &self.ids
    }

    fn layouts(&self) -> Vec<SystemLayout> {
        self.layouts.clone()
    }

    fn add_layouts(&self, buffer: &mut Vec<SystemLayout>, base_offset: usize) {
        buffer.extend(self.layouts.iter().map(|x| SystemLayout {
            offset: x.offset + base_offset,
            len: x.len,
        }));
    }

    fn add_links_to(&self, to_id: usize, base_offset: usize, links: &mut Vec<SystemLink>)
    where
        Out: KnownLayout,
    {
        for (i, SystemLayout { offset, len }) in self.layouts.iter().enumerate() {
            links.push(SystemLink {
                from_system_idx: self.ids[i],
                to_system_idx: to_id,
                to_input_offset: base_offset + offset,
                num_bytes: *len,
            });
        }
    }
}

pub trait System {
    type Input;
    type Output;

    fn update(&mut self, time: f64, input: &Self::Input, output: &mut Self::Output) -> f64;
    /// `true` if system has internal state / delay that makes it break algebraic loops.
    fn is_stateful(&self) -> bool {
        false
    }
}

pub trait RawSystem {
    fn raw_update(&mut self, time: f64, input: Cow<[u8]>, output: &mut [u8]) -> f64;
    fn is_stateful(&self) -> bool;
}

impl<Sys> RawSystem for Sys
where
    Sys: System,
    Sys::Input: FromBytes + Immutable + KnownLayout,
    Sys::Output: FromBytes + IntoBytes + KnownLayout,
{
    fn raw_update(&mut self, time: f64, input: Cow<[u8]>, output: &mut [u8]) -> f64 {
        Sys::update(
            self,
            time,
            Sys::Input::ref_from_bytes(input.as_ref()).expect("Could not read input"),
            Sys::Output::mut_from_bytes(output).expect("Could not read output"),
        )
    }

    fn is_stateful(&self) -> bool {
        System::is_stateful(self)
    }
}

#[derive(Debug, Clone)]
pub struct SystemLink {
    from_system_idx: usize,
    to_system_idx: usize,
    to_input_offset: usize,
    num_bytes: usize,
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
        let mut input_buffers: Vec<Vec<u8>> = (0..self.systems.len())
            .map(|idx| {
                let links = &links_to_node[idx];

                // Only allocate if we have mode than one input link
                if links.len() > 1 {
                    let input_size = links
                        .iter()
                        .map(|l| l.to_input_offset + l.num_bytes)
                        .max()
                        .unwrap_or(0);
                    vec![0u8; input_size]
                } else {
                    Vec::new()
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

                    input_cow = Cow::Borrowed(input_buffer.as_slice());
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
        In: IntoBytes,
        Out: FromBytes,
    {
        self.systems.push(Box::new(sys));
        self.outputs.push(vec![0u8; size_of::<Out>()]);

        SystemRef {
            id: [self.systems.len() - 1],
            _io: PhantomData,
        }
    }

    pub fn link<Data>(&mut self, from: &impl SystemOut<Data>, to: &impl SystemIn<Data>)
    where
        Data: KnownLayout,
    {
        assert!(has_unique_elements(from.ids()));
        assert!(!from.ids().contains(&to.id()));
        from.add_links_to(to.id(), 0, &mut self.links)
    }

    pub fn mux<In1, In2>(
        &mut self,
        from1: &impl SystemOut<In1>,
        from2: &impl SystemOut<In2>,
    ) -> SystemMux<Pair<In1, In2>>
    where
        In1: Default + Clone + 'static,
        In2: Default + Clone + 'static,
    {
        let ids = from1
            .ids()
            .iter()
            .chain(from2.ids().iter())
            .copied()
            .collect();

        let mut layouts = from1.layouts();
        let len = layouts.iter().map(|x| x.len).sum();
        from2.add_layouts(&mut layouts, len);

        SystemMux {
            ids,
            layouts,
            _io: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, IntoBytes};

    #[test]
    fn test_pair() {
        let a = Pair(255u8, i64::MAX);
        let b = a.as_bytes();
        let c = Pair::ref_from_bytes(b).unwrap();

        let d = a.0.as_bytes();
        let e = a.1.as_bytes();
        let f: Vec<_> = d.iter().chain(e).copied().collect();
        let g = Pair::ref_from_bytes(&f).unwrap();

        assert_eq!(&a, c);
        assert_eq!(&a, g);
    }

    #[test]
    fn test_tuple() {
        let a = (255u8, i64::MAX);

        let b = a.as_bytes();
    }
}
