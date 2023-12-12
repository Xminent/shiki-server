ARG RUST_VERSION=1.74.0
FROM alpine:3.17 AS build
WORKDIR /app

ENV CMAKE_BUILD_TYPE=Release
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV OPENSSL_LIB_DIR=/usr/lib
ENV OPENSSL_STATIC=1
ENV PIP_ROOT_USER_ACTION=ignore
ENV RUSTFLAGS="-C target-feature=-crt-static"

# TODO: Make a fully static binary. May or may not be needed.
# clang-static llvm-static would probably be needed.

RUN --mount=type=bind,source=lib,target=lib \
    --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=sys,target=sys \
    --mount=type=bind,source=build.rs,target=build.rs \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
apk --no-cache --update add autoconf automake bash build-base clang-dev cmake git help2man llvm-dev openssl-dev openssl-libs-static pkgconfig python3 py3-pip rustup
rustup-init -y
source "$HOME/.cargo/env"
echo "cython<3" > /tmp/constraint.txt
PIP_CONSTRAINT=/tmp/constraint.txt pip install -v "conan==1.60.0"
conan profile new --detect default
conan profile update settings.compiler.libcxx=libstdc++11 default
cargo build --locked --release --verbose
cp ./target/release/shiki-server /bin/shiki-server
EOF

FROM alpine:3.17 AS final

ARG UID=10001

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser \
    && apk add --no-cache --update libgcc
USER appuser

COPY --from=build /bin/shiki-server /bin/

EXPOSE 8080

CMD ["/bin/shiki-server"]
