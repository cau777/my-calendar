# ---- build: static musl binary with the PWA embedded ----
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src/server
# cache dependencies
COPY server/Cargo.toml server/Cargo.lock ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && mkdir -p ../web && touch ../web/index.html \
 && cargo build --release && rm -rf src
COPY server/src ./src
COPY web ../web
RUN touch src/main.rs && cargo build --release && mkdir -p /out/data && cp target/release/my-calendar /out/

# ---- runtime: nothing but the binary ----
FROM scratch
COPY --from=build --chown=65534:65534 /out /app
USER 65534:65534
ENV PORT=8080 DATA_DIR=/app/data
VOLUME /app/data
EXPOSE 8080
ENTRYPOINT ["/app/my-calendar"]
