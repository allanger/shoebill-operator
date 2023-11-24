FROM rust:1.74.0-alpine3.18 AS builder
RUN apk update && apk add --no-cache musl-dev
WORKDIR /src
COPY . /src
RUN cargo build --release

FROM alpine
COPY --from=builder /src/target/release/shoebill /shoebill
ENTRYPOINT /shoebill
