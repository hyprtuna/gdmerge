#![no_main]

use libfuzzer_sys::fuzz_target;

// The tokenizer must terminate and stay inside the input for any string.
fuzz_target!(|data: &str| {
    tscn::fuzz::tokenize(data);
});
