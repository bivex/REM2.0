//! Level 2: Control Flow Stress Test
//! Target: Extract the logic inside the loop.
//! Challenges:
//! - `break` and `continue` target the outer loop.
//! - Conditional returns.
//! - Error handling within the block.

fn main() {
    let _ = level_02_control_flow(vec![Some(1), None, Some(3)]);
}

pub fn level_02_control_flow(items: Vec<Option<i32>>) -> Result<i32, String> {
    let mut sum = 0;

    for (i, item) in items.into_iter().enumerate() {
        // --- EXTRACT START ---
        let val = match item {
            Some(v) => v,
            None => continue, // Should trigger 'continue' in caller
        };

        if val < 0 {
            return Err(format!("Negative value at index {}", i)); // Early return from function
        }

        if sum > 100 {
            break; // Should trigger 'break' in caller
        }

        sum += val;
        // --- EXTRACT END ---
    }

    Ok(sum)
}
