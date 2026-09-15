use std::collections::HashMap;

fn main() {
    let units: HashMap<u32, u32> = HashMap::new();
    let mut ids: Vec<_> = units.keys().collect();
    ids.sort();
    for id in ids {
        println!("{id}");
    }
}
