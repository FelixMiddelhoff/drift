use rayon::prelude::*;

fn main() {
    let units = vec![1, 2, 3];
    units.par_iter().for_each(|u| {
        let _ = u;
    });
}
