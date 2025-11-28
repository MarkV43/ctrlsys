use ctrlsys::{
    pool::SystemPool,
    system::{
        System,
        discrete::{DiscreteSystem, holder::ZeroOrderHold},
    },
};
use std::{f64, time::Instant};

struct Filter;

impl<'s> System<'s> for Filter {
    type Input<'a> = &'a f64;
    type Output<'a> = &'a mut f64;

    fn update(&mut self, time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64 {
        *output = *output * 0.95 + input * 0.05;
        // println!("f: {:.02}", *output);
        (time * 10.0 + 1.).floor() * 0.1
    }
}

struct Input {
    step_time: f64,
    value: f64,
}

impl<'s> System<'s> for Input {
    type Input<'a> = ();
    type Output<'a> = &'a mut f64;

    fn update(&mut self, time: f64, _: (), output: Self::Output<'s>) -> f64 {
        if time >= self.step_time {
            self.value = 1.0;
            *output = 1.0;
            f64::INFINITY
        } else {
            *output = 0.0;
            self.step_time
        }
    }
}

struct Test;

impl<'s> System<'s> for Test {
    type Input<'a> = (&'a f64, &'a f64);
    type Output<'a> = &'a mut f64;

    fn update(&mut self, _time: f64, input: Self::Input<'s>, output: Self::Output<'s>) -> f64 {
        *output = input.0 - input.1;
        // println!("test: {output}");
        f64::INFINITY
    }
}

struct DiscreteTest {
    state: [f64; 2],
}

impl DiscreteSystem for DiscreteTest {
    type Input<'a> = &'a f64;
    type Output<'a> = &'a mut f64;

    fn calculate(&mut self, _time: f64, input: Self::Input<'_>, output: Self::Output<'_>) {
        self.state[1] += 0.1 * self.state[0] + input;
        self.state[0] += input;

        *output = self.state[1];
    }

    fn timestep(&self) -> f64 {
        0.05
    }
}

fn main() {
    let mut pool = SystemPool::new();

    let filter = pool.add_system(Filter);

    let inp = pool.add_system(Input {
        step_time: 0.15,
        value: 0.0,
    });

    pool.link(inp, &filter);

    let mux = (&inp, &filter);

    let test = pool.add_system(Test);

    pool.link(mux, &test);

    let start = Instant::now();

    let discr = DiscreteTest { state: [0.0; 2] };

    let dsys = discr.with_holder(ZeroOrderHold::new());

    let discr = pool.add_system(dsys);

    pool.link(test, &discr);

    pool.simulate(10.0, 0.02).unwrap();

    let dur = start.elapsed();

    println!("Elapsed: {dur:?}");
}
