# The monitoring collector.
#
# Built with `context: ./monitoring`, which works because `erno new` rewrites
# the `erno` dependency in monitoring/Cargo.toml to the git (or absolute path)
# form when it scaffolds the project — a relative `../api` would escape the
# build context.
#
# No {{name}} substitution: the collector is framework code, and the project's
# identity lives in the release name and the image repository, not the binary.
FROM rust:1.88 AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin erno-monitoring

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/erno-monitoring /usr/local/bin/erno-monitoring
COPY config/ /app/config/
WORKDIR /app
# The status publisher writes here; the chart mounts a volume over it.
RUN mkdir -p /app/status
EXPOSE 3001
CMD ["erno-monitoring"]
