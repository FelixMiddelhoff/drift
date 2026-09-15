use std::collections::HashMap;

fn main() {
    let units: HashMap<u32, u32> = HashMap::new();

    for (_id, _unit) in units.iter() {
        println!("flagged");
    }
}
