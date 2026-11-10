<!-- markdownlint-disable MD041 -- First line in a file should be a top-level heading -->

# Fuzz corpus

One directory per fuzz target, named the way the target names itself. Every file in a directory is replayed on each run before the generated cases, and is also used as a parent for the mutator.

There are 2 kinds of file here.

- Seeds: a realistic input, so the mutator has something realistic to start from. Most of what's here now is a seed.

- A case that once broke something. Save it under a name that says what it was, and it's replayed forever afterwards. A defect that also has a plain unit test doesn't need a file here as well. This directory is for the ones only the fuzzer can express.

Everything here is read-only data. Nothing but the fuzz targets may open one of these files, and a seed that's a valid settings file is the easy way to forget that. `silkterm --config cicd/tests/fuzz-corpus/config/shipped-default.shcl` rewrites it the way a launch rewrites anyone's config, and the seed stops being the case it was saved as. Copy a seed somewhere else to look at it. The pipeline hashes this directory either side of the test stage, so a run that writes through one fails.

The engine and the targets are described at the top of `source/src/fuzz.rs`. The targets themselves are next to the code they test, in a `mod fuzz` inside that module's tests.

To reproduce one case, take the seed from the failure and run that alone:

~~~sh
SILK_FUZZ_SEED=1234 cargo test <target name>
~~~

`SILK_FUZZ_SECS` sets how long each target gets. A plain `cargo test` leaves it unset and every target takes a fraction of a second. The pipeline sets it for a real soak. Whatever the budget, a target always runs its first 10 seeds, so a busy box tests the same cases as an idle one.
