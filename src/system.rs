use std::borrow::Cow;

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
    fn input_alignment(&self) -> usize;
}

impl<Sys> RawSystem for Sys
where
    Sys: System,
{
    fn raw_update(&mut self, time: f64, input: Cow<[u8]>, output: &mut [u8]) -> f64 {
        let i_ptr = input.as_ref() as *const [u8] as *const Sys::Input;
        let o_ptr = output.as_mut() as *mut [u8] as *mut Sys::Output;

        let i_ref = unsafe { &*i_ptr };
        let o_ref = unsafe { &mut *o_ptr };

        Sys::update(self, time, i_ref, o_ref)
    }

    fn is_stateful(&self) -> bool {
        System::is_stateful(self)
    }

    fn input_alignment(&self) -> usize {
        align_of::<Sys::Input>()
    }
}
