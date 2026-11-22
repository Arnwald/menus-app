# Image en deux temps : compilation, puis un binaire unique avec l'interface
# (web/) et la base de départ (seed/) embarquées.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
COPY seed ./seed
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/*

ENV TZ=Europe/Zurich \
    MENUS_DATA_DIR=/data \
    MENUS_PORT=8080 \
    MENUS_HOST=0.0.0.0

COPY --from=builder /build/target/release/menus /usr/local/bin/menus

VOLUME ["/data"]
EXPOSE 8080

ENTRYPOINT ["menus"]
CMD ["serve"]
