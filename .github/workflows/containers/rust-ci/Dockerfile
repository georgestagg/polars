ARG BASE=ghcr.io/georgestagg/llvm-project:wasm-ci
FROM $BASE
ARG EMSCRIPTEN_VERSION=3.1.58

WORKDIR /opt

RUN apt-get update && \
  apt-get install -y  --no-install-recommends build-essential git curl unzip \
    cmake ninja-build lsb-release python3 xz-utils ca-certificates python3 \
    python3-venv && apt-get clean

RUN curl -L "https://sh.rustup.rs" | sh -s -- --default-toolchain none -y
ENV PATH="/root/.cargo/bin:$PATH"

RUN rustup toolchain install nightly

RUN git clone --depth=1 https://github.com/emscripten-core/emsdk.git /opt/emsdk
RUN /opt/emsdk/emsdk install "${EMSCRIPTEN_VERSION}" && \
  /opt/emsdk/emsdk activate "${EMSCRIPTEN_VERSION}"

ENV PATH="/opt/emsdk:/opt/emsdk/upstream/emscripten:$PATH"
ENV EMSDK="/opt/emsdk"

RUN git clone --depth=1 https://github.com/rust-lang/rust.git

RUN curl -O -L https://github.com/georgestagg/llvm-project/archive/refs/heads/wasm-ci.tar.gz && \
  tar -xf wasm-ci.tar.gz && \
  rm wasm-ci.tar.gz

RUN rm -rf rust/src/llvm-project && mv llvm-project-wasm-ci rust/src/llvm-project

COPY config.toml rust/config.toml
RUN cd rust && ./x build --stage 2 library

FROM $BASE
COPY --from=0 /opt/emsdk /opt/emsdk
COPY --from=0 /opt/llvm /opt/llvm
COPY --from=0 /opt/rust/build/x86_64-unknown-linux-gnu/stage2 /opt/rust/build/x86_64-unknown-linux-gnu/stage2
COPY --from=0 /opt/rust/build/x86_64-unknown-linux-gnu/stage2-std /opt/rust/build/x86_64-unknown-linux-gnu/stage2-std
RUN ln -s /opt/rust/build/x86_64-unknown-linux-gnu /opt/rust/build/host

ENV PATH="/opt/emsdk:/opt/emsdk/upstream/emscripten:$PATH"
ENV EMSDK="/opt/emsdk"
ENV EM_NODE_JS="/opt/emsdk/node/18.20.3_64bit/bin/node"
ENV EM_LLVM_ROOT="/opt/llvm/bin"

RUN apt-get update && \
  apt-get install -y  --no-install-recommends build-essential git curl unzip \
    cmake ninja-build lsb-release python3 xz-utils ca-certificates python3 \
    python3-venv && apt-get clean

SHELL ["/bin/bash", "-c"]
