FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && mkdir templates tests
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src templates tests
COPY src/ src/
COPY templates/ templates/
COPY tests/ tests/
RUN cargo build --release

FROM alpine:3.21
RUN apk add --no-cache ca-certificates tzdata su-exec
COPY --from=builder /app/target/release/pastebox /usr/local/bin/pastebox
COPY templates/ /usr/local/share/pastebox/templates/
COPY docker-entrypoint.sh /
RUN adduser -D -h /paste-data pastebox
ENV PASTEBOX_DATA_DIR=/paste-data
EXPOSE 8080
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["pastebox"]
