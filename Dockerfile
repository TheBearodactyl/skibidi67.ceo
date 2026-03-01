FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs
RUN cargo build --release
RUN rm src/main.rs

COPY static ./static
COPY synthemes ./synthemes
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM alpine

RUN apk add --no-cache ca-certificates ffmpeg

WORKDIR /app

COPY --from=builder /app/target/release/skibidi67 .
COPY Rocket.toml .
COPY templates ./templates
COPY static ./static
COPY synthemes ./synthemes

RUN mkdir -p uploads

EXPOSE 8090
ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8090

CMD ["./skibidi67"]
