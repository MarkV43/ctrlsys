use ctrlsys::system::{Pair, System, SystemPool};
use std::f64;

struct Filter;

impl System for Filter {
    type Input = f64;
    type Output = f64;

    fn update(&mut self, time: f64, input: &f64, output: &mut f64) -> f64 {
        *output = *output * 0.95 + input * 0.05;
        println!("f: {:.02}", *output);
        (time * 10.0 + 1.).floor() * 0.1
    }
}

struct Input {
    step_time: f64,
    value: f64,
}

impl System for Input {
    type Input = ();
    type Output = f64;

    fn update(&mut self, time: f64, _: &(), output: &mut f64) -> f64 {
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

impl System for Test {
    type Input = Pair<f64, f64>;
    type Output = f64;

    fn update(&mut self, _time: f64, input: &Self::Input, output: &mut Self::Output) -> f64 {
        *output = input.0 - input.1;
        println!("test: {output}");
        f64::INFINITY
    }
}

fn main() {
    let mut pool = SystemPool::new();

    let filter = pool.add_system(Filter);

    let inp = pool.add_system(Input {
        step_time: 0.15,
        value: 0.0,
    });

    pool.link(&inp, &filter);

    let mux = pool.mux(&inp, &filter);

    let test = pool.add_system(Test);

    pool.link(&mux, &test);

    pool.simulate(10.0, 0.2).unwrap();
}
