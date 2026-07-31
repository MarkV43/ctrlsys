use std::ops::{Add, Mul};

use crate::system::SystemDataOut;

pub trait Holder {
    type Payload: ?Sized;
    fn update_input(&mut self, time: f64, input: &Self::Payload);
    fn update_output<'a, Out>(&self, time: f64, output: Out)
    where
        Out: SystemDataOut<'a, Payload = Self::Payload>;
}

pub struct ZeroOrderHold<Data> {
    last_time: f64,
    data: Data,
}

impl<Data> ZeroOrderHold<Data> {
    #[must_use]
    pub fn new() -> Self
    where
        Data: Default,
    {
        Self {
            last_time: f64::MIN,
            data: Data::default(),
        }
    }
}

impl<Data> Default for ZeroOrderHold<Data>
where
    Data: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Data> Holder for ZeroOrderHold<Data>
where
    Data: Clone,
{
    type Payload = Data;

    fn update_input(&mut self, time: f64, input: &Self::Payload) {
        self.data = input.clone();
        self.last_time = time;
    }

    fn update_output<'a, Out>(&self, _time: f64, mut output: Out)
    where
        Out: SystemDataOut<'a, Payload = Self::Payload>,
    {
        output.copy_from_payload(&self.data);
    }
}

pub struct FirstOrderHold<Data> {
    last_time: f64,
    last_data: Data,
    curr_time: f64,
    curr_data: Data,
}

impl<Data> FirstOrderHold<Data> {
    #[must_use]
    pub fn new() -> Self
    where
        Data: Default,
    {
        Self {
            last_time: f64::MIN,
            last_data: Data::default(),
            curr_time: f64::MIN,
            curr_data: Data::default(),
        }
    }
}

impl<Data> Default for FirstOrderHold<Data>
where
    Data: Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Data> Holder for FirstOrderHold<Data>
where
    Data: Clone + Add<Output = Data>,
    for<'a> &'a Data: Mul<f64, Output = Data>,
{
    type Payload = Data;

    fn update_input(&mut self, time: f64, input: &Self::Payload) {
        #[expect(
            clippy::float_cmp,
            reason = "The discrete-system hardening phase owns this. The comparison \
                      is against the `f64::MIN` sentinel written at construction, \
                      not a computed value, so exact equality is correct as written. \
                      It is left alone here because that phase reworks first-sample \
                      handling in this impl — `update_output` divides by \
                      `curr_time - last_time`, which is zero on the first sample and \
                      yields NaN — and any change to how initialisation is detected \
                      belongs with that fix, not with a lint sweep. See \
                      specs/roadmap.md, Phase 8 at time of writing."
        )]
        let initialized = self.curr_time != f64::MIN;

        // TODO get rid of some of these clones somehow?
        if initialized {
            self.last_time = self.curr_time;
            self.last_data = self.curr_data.clone();
        } else {
            self.last_time = time;
            self.last_data = input.clone();
        }

        self.curr_time = time;
        self.curr_data = input.clone();
    }

    fn update_output<'a, Out>(&self, time: f64, mut output: Out)
    where
        Out: SystemDataOut<'a, Payload = Self::Payload>,
    {
        let dt = self.curr_time - self.last_time;
        let t_base = time - self.curr_time;
        debug_assert!(t_base < dt);

        let prop = t_base / dt;

        let payload = &self.last_data * prop + &self.curr_data * (1.0 - prop);
        output.copy_from_payload(&payload);
    }
}
