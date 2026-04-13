//! Level 1: Mutability & Ownership Stress Test
//! Target: Extract the logic inside the marked block.
//! Challenges:
//! - Multiple mutable borrows.
//! - Partial moves.
//! - Variable shadowing.

fn main() {
    level_01_mut_ownership();
}

pub fn level_01_mut_ownership() {
    let mut x = 10;
    let mut y = vec![1, 2, 3];
    let z = String::from("hello");

    println!("Initial: x={}, y={:?}, z={}", x, y, z);

    // --- EXTRACT START ---
    x += 1;
    y.push(4);
    let z_len = z.len();
    let first = y.remove(0);
    let msg = format!("{} world, x is now {}, first was {}", z, x, first);
    println!("{}", msg);
    // --- EXTRACT END ---

    println!("Final: x={}, y={:?}, z_len={}", x, y, z_len);
}
