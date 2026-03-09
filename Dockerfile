FROM rustlang/rust:nightly

WORKDIR /workspace

RUN rustup component add rust-src rustc-dev llvm-tools-preview

COPY . /workspace

RUN cargo install --path .

ENTRYPOINT ["bash"]
