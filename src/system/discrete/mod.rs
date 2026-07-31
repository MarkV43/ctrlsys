pub mod holder;

use std::fmt::Debug;

use crate::system::{System, SystemDataIn, SystemDataOut, discrete::holder::Holder};

pub trait DiscreteSystem {
    type Input<'a>: SystemDataIn<'a>;
    type Output<'a>: SystemDataOut<'a>;

    /// Compute this step's output.
    ///
    /// `output` is taken by `&mut` rather than by value so that a holder wrapping this
    /// system can lend the output out and still use it afterwards — `HeldSystem` reads
    /// the computed value back through `payload_ref` and then overwrites it with the
    /// held value. Passing by value would force the holder to duplicate the view, which
    /// for the usual case where `Output` is `&mut T` means duplicating a mutable
    /// reference through a raw pointer.
    ///
    /// Where `Output` is `&mut T`, write through it as `**output = value`.
    fn calculate(&mut self, time: f64, input: Self::Input<'_>, output: &mut Self::Output<'_>);

    fn timestep(&self) -> f64;

    fn with_holder<'a, Hol>(self, holder: Hol) -> HeldSystem<Self, Hol>
    where
        Self: Sized,
        Hol: Holder<Payload = <Self::Output<'a> as SystemDataOut<'a>>::Payload>,
    {
        HeldSystem {
            system: self,
            holder,
            last_time: f64::MIN,
        }
    }
}

pub struct HeldSystem<Sys, Hol> {
    system: Sys,
    holder: Hol,
    last_time: f64,
}

impl<'s, Sys, Hol> System<'s> for HeldSystem<Sys, Hol>
where
    Sys: DiscreteSystem,
    for<'b> Sys::Output<'b>: SystemDataOut<'b>,
    Sys::Output<'s>: Debug,
    Hol: Holder<Payload = <Sys::Output<'s> as SystemDataOut<'s>>::Payload>,
{
    type Input<'a> = Sys::Input<'a>;
    type Output<'a> = Sys::Output<'a>;

    fn update(&mut self, time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64 {
        let req_dt = self.system.timestep();
        let mut dt = time - self.last_time;

        #[expect(
            clippy::float_cmp,
            reason = "The discrete-system hardening phase owns this, as with the \
                      matching sentinel test in `holder.rs`. Compared against the \
                      `f64::MIN` sentinel rather than a computed value, so exact \
                      equality is correct; changing it belongs with that phase's \
                      first-sample rework. See specs/roadmap.md, Phase 8 at time of \
                      writing."
        )]
        if self.last_time == f64::MIN {
            self.last_time = time;
            dt = time - self.last_time;
        }

        let dt = dt;
        // If the request time
        assert!(
            dt - req_dt < req_dt * 1e-5,
            "Requested event was not triggered"
        );

        if dt + 1e-10 >= req_dt {
            let mut output = output;

            // Lend the output to `calculate` and take it back, rather than duplicating
            // it. This used to be a `ptr::read` through a raw pointer, which copied the
            // `&mut` bit-for-bit so that two handles to the same place existed at once.
            self.system.calculate(time, input, &mut output);
            self.holder.update_input(time, output.payload_ref());
            self.holder.update_output(time, output);
            self.last_time += req_dt;
            self.last_time + 2.0 * req_dt
        } else {
            self.last_time + req_dt
        }
    }
}
