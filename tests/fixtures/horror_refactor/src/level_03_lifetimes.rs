//! Level 3: Lifetime Labyrinth
//! Target: Extract the marked block.
//! Challenges:
//! - Multiple input lifetimes.
//! - Returning a reference that depends on the input references.
//! - Lifetime elision vs explicit lifetimes.

pub struct Container<'a> {
    data: &'a str,
}

fn main() {
    let c = Container { data: "hello_world" };
    let _ = level_03_lifetimes(&c, "hello_");
}

pub fn level_03_lifetimes<'a, 'b>(c: &'a Container<'a>, prefix: &'b str) -> &'a str {
    let mut result = "";

    // --- EXTRACT START ---
    if c.data.starts_with(prefix) {
        result = &c.data[prefix.len()..];
    } else {
        result = "default";
    }
    // --- EXTRACT END ---

    result
}
