pub mod discrete;

use std::marker::PhantomData;

pub trait System<'s> {
    type Input<'a>: SystemDataIn<'a>;
    type Output<'a>: SystemDataOut<'a>;

    fn update(&mut self, time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64;
    /// `true` if system has internal state / delay that makes it break algebraic loops.
    fn is_stateful(&self) -> bool {
        false
    }
}

pub trait RawSystem {
    fn raw_update<'a>(&mut self, time: f64, input: &[&'a [u8]], output: &mut [&'a mut [u8]])
    -> f64;
    fn is_stateful(&self) -> bool;
    fn output_alignment(&self) -> usize;
    fn output_size(&self) -> usize;
}

impl<Sys> RawSystem for Sys
where
    for<'s> Sys: System<'s>,
{
    fn is_stateful(&self) -> bool {
        System::is_stateful(self)
    }

    fn output_alignment(&self) -> usize {
        align_of::<Sys::Output<'_>>()
    }

    fn output_size(&self) -> usize {
        size_of::<Sys::Output<'_>>()
    }

    fn raw_update<'a>(
        &mut self,
        time: f64,
        input: &[&'a [u8]],
        output: &mut [&'a mut [u8]],
    ) -> f64 {
        let i_ref = unsafe { Sys::Input::from_slices(input) };
        let o_ref = unsafe { Sys::Output::from_slices_mut(output) };

        Sys::update(self, time, i_ref, o_ref)
    }
}

pub trait SystemDataIn<'a> {
    type Payload: ?Sized;
    unsafe fn from_slices(slices: &[&'a [u8]]) -> Self;
}

pub trait SystemDataOut<'a> {
    type Payload: ?Sized;
    unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self;
    fn copy_from_payload(&mut self, payload: &Self::Payload);
    fn payload_ref(&self) -> &Self::Payload;
}

impl<'a> SystemDataIn<'a> for () {
    type Payload = ();

    unsafe fn from_slices(_: &[&'a [u8]]) -> Self {}
}

impl<'a> SystemDataOut<'a> for () {
    type Payload = PhantomData<()>; // TODO replace with ! when stabilized

    unsafe fn from_slices_mut(_: &mut [&'a mut [u8]]) -> Self {}
    fn copy_from_payload(&mut self, _: &Self::Payload) {}

    fn payload_ref(&self) -> &Self::Payload {
        &PhantomData
    }
}

impl<'a, T> SystemDataIn<'a> for &'a T {
    type Payload = T;

    unsafe fn from_slices(slices: &[&'a [u8]]) -> Self {
        debug_assert!(
            !slices.is_empty(),
            "System expected 1 input, found {}",
            slices.len(),
        );

        let ptr = slices[0].as_ptr() as *const T;
        unsafe { &*ptr }
    }
}

impl<'a, T: Clone> SystemDataOut<'a> for &'a mut T {
    type Payload = T;

    unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self {
        debug_assert!(!slices.is_empty());

        let ptr = slices[0].as_mut_ptr() as *mut T;
        unsafe { &mut *ptr }
    }

    fn copy_from_payload(&mut self, payload: &Self::Payload) {
        **self = payload.clone();
    }

    fn payload_ref(&self) -> &Self::Payload {
        &*self
    }
}

impl<'a, T, U> SystemDataIn<'a> for (&'a T, &'a U) {
    type Payload = (T, U);

    unsafe fn from_slices(slices: &[&[u8]]) -> Self {
        debug_assert!(
            slices.len() == 2,
            "System expected 2 inputs, found {}",
            slices.len()
        );

        let t_ptr = slices[0].as_ptr() as *const T;
        let u_ptr = slices[1].as_ptr() as *const U;
        unsafe { (&*t_ptr, &*u_ptr) }
    }
}

// impl<'a, T, U> SystemDataOut<'a> for (&'a mut T, &'a mut U) {
//     unsafe fn from_slices_mut(slices: &mut [&'a mut [u8]]) -> Self {
//         debug_assert!(slices.len() >= 2);

//         let t_ptr = slices[0].as_mut_ptr() as *mut T;
//         let u_ptr = slices[1].as_mut_ptr() as *mut U;
//         unsafe { (&mut *t_ptr, &mut *u_ptr) }
//     }
// }
