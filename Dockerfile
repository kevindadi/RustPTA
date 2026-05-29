FROM rustlang/rust:nightly

WORKDIR /workspace

RUN apt-get update \
    && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

# RustPTA (default nightly) + Miri
RUN rustup component add rust-src rustc-dev llvm-tools-preview miri

# lockbud and AtomVChecker each pin a specific nightly
RUN rustup toolchain install nightly-2026-02-07 --component rust-src rustc-dev llvm-tools-preview \
    && rustup toolchain install nightly-2023-03-09 --component rust-src rustc-dev llvm-tools-preview

# External tools live alongside RustPTA under tools/
RUN mkdir -p /workspace/tools

RUN git clone --depth 1 https://github.com/CodeSentryAI/lockbud.git /workspace/tools/lockbud \
    && cargo +nightly-2026-02-07 install --path /workspace/tools/lockbud --locked

RUN git clone --depth 1 https://github.com/AtomVChecker/rust-atomic-study.git /workspace/tools/rust-atomic-study \
    && cargo +nightly-2023-03-09 install --path /workspace/tools/rust-atomic-study/section-5-detection/AtomVChecker --locked

COPY . /workspace
RUN cargo install --path .

ENTRYPOINT ["bash"]
