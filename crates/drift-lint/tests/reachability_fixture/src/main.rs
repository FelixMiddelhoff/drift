use std::collections::HashMap;

fn tick() {
    helper();
    physics_integrate(1.0, 2.0, 3.0);
}

fn helper() {
    let units: HashMap<u32, u32> = HashMap::new();
    for (_id, _unit) in units.iter() {
        // reachable from tick() -> should be flagged (drift::hashmap_iter)
        println!("reachable violation");
    }

    let a = 1.0_f32;
    let b = 2.0_f32;
    let c = 3.0_f32;
    let _sum = a + b + c; // reachable, not fixed-step-exempt -> should be flagged (drift::float_outside_fixed_step)
}

// Listed in dylint.toml's fixed_step_functions -> exempt even though
// reachable from tick().
fn physics_integrate(a: f32, b: f32, c: f32) -> f32 {
    a + b + c // exempt -> should NOT be flagged
}

fn unreachable_fn() {
    let units: HashMap<u32, u32> = HashMap::new();
    for (_id, _unit) in units.iter() {
        // not reachable from tick() -> should NOT be flagged
        println!("unreachable violation");
    }

    let a = 1.0_f32;
    let b = 2.0_f32;
    let _sum = a + b; // not reachable -> should NOT be flagged
}

fn main() {
    tick();
    unreachable_fn();
}
