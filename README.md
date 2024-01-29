Static deadlock detection for Rust Programs.

## Install
Currently supports rustc nightly-2023-09-13
```
$ rustup component add rust-src
$ rustup component add rustc-dev
$ rustup component add llvm-tools-preview
$ cargo install --path .
```
## Example
Test 

```
$ cd toys/intra; cargo clean; cargo pta
```
$ cd YourProject; cargo clean; cargo pta
$ cd YourProject; cargo clean; cargo pta -k deadlock
$ cd YourProject; cargo clean; cargo pta -k atomicity_violation
$ cd YourProject; cargo clean; cargo pta -k memory
$ cd YourProject; cargo clean; cargo pta -k all
```

