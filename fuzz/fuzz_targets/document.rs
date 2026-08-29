#![no_main]

use libfuzzer_sys::fuzz_target;

// The invariant the whole tool rests on: a document that parses re-serialises
// to exactly the bytes it came from.
fuzz_target!(|data: &str| {
    tscn::fuzz::parse_document(data);
});
