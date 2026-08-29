#![no_main]

use libfuzzer_sys::fuzz_target;

// Parsing a variant literal must either fail or produce a value that can be
// canonicalised, which is what every merge comparison relies on.
fuzz_target!(|data: &str| {
    tscn::fuzz::parse_value(data);
});
