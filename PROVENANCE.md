# Clean-source provenance policy

This repository is independently authored under Apache-2.0. Public behavior
may be compatible with other Agent Seat implementations, but their source is
not an implementation input.

Every pull request must identify one provenance class:

1. original work written for this repository from its public specification;
2. a dependency or asset with its exact upstream URL, license, and purpose; or
3. a standards-derived fact with the exact public source named.

Copied or mechanically translated implementation code, comments, test
language, fixtures, schemas, or prose are rejected. Ideas from another product
must first be stated as observable requirements, then implemented afresh.

## Initial tree

The initial Rust sources, manifests, workflow, ownership rules, policies, and
project prose were written for this repository after it was created. The only
verbatim standard documents are:

- `LICENSE`: Apache License 2.0 from the Apache Software Foundation;
- `DCO.txt`: Developer Certificate of Origin 1.1 from The Linux Foundation.

GitHub's checkout action is referenced as a CI dependency and is not vendored.
The Rust toolchain is installed directly by `rustup`. No file in the initial
tree was copied from Nobox.
