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
RUN cargo build --release --bin {{name}}

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/{{name}} /usr/local/bin/{{name}}
COPY config/ /app/config/
WORKDIR /app
EXPOSE 3000
CMD ["{{name}}"]
