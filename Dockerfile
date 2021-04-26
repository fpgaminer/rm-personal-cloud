FROM rust:1.51 as builder

WORKDIR /usr/src/rm-personal-cloud
COPY src ./src
COPY Cargo.* ./
COPY schema.sql ./

RUN cargo install --path .


FROM debian:buster-slim

COPY --from=builder /usr/local/cargo/bin/rm-personal-cloud /usr/local/bin/rm-personal-cloud

ENTRYPOINT ["rm-personal-cloud"]
CMD ["--bind", "0.0.0.0", "--https-port", "8084", "--ssl-cert", "/ssl/cert.pem", "--ssl-key", "/ssl/key.pem", "--hostname", "local.appspot.com", "--db", "/data/db.sqlite"]