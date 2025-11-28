pub mod holder;

use std::fmt::Debug;

use crate::system::{System, SystemDataIn, SystemDataOut, discrete::holder::Holder};

pub trait DiscreteSystem {
    type Input<'a>: SystemDataIn<'a>;
    type Output<'a>: SystemDataOut<'a>;

    fn calculate(&mut self, time: f64, input: Self::Input<'_>, output: Self::Output<'_>);

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
            let output_bytes = &output as *const Self::Output<'s>;
            let output_clone = unsafe { output_bytes.read() };

            self.system.calculate(time, input, output_clone);
            self.holder.update_input(time, output.payload_ref());
            self.holder.update_output(time, output);
            self.last_time += req_dt;
            self.last_time + 2.0 * req_dt
        } else {
            self.last_time + req_dt
        }
    }
}
