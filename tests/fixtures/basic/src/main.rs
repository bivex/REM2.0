fn main() {
    let mut accumulator = 0;
    let multiplier = 5;

    // We will extract the following loop into a function.
    // accumulator is mutated inside AND outside -> MutRef
    // multiplier is read inside -> SharedRef

    // -- EXTRACT START --
    let mut local_counter = 0;
    loop {
        if local_counter >= 10 {
            break;
        }
        accumulator += multiplier;
        local_counter += 1;
    }
    // -- EXTRACT END --

    println!("Result: {}", accumulator);
}
