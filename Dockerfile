# Build the server
FROM rust:1.51 as builder

WORKDIR /usr/src/rm-personal-cloud
COPY src ./src
COPY Cargo.* ./
COPY build.rs ./
COPY schema.sql ./
COPY admin-webapp/*.json ./admin-webapp/
COPY admin-webapp/*.js ./admin-webapp/
COPY admin-webapp/src ./admin-webapp/src
COPY admin-webapp/dist/index.html ./admin-webapp/dist/index.html

RUN apt-get update
RUN apt-get -y install curl gnupg
RUN curl -fsSL https://deb.nodesource.com/setup_16.x | bash -
RUN apt-get -y install nodejs

RUN cargo install --path .


# Build the final image
FROM debian:buster-slim

COPY --from=builder /usr/local/cargo/bin/rm-personal-cloud /usr/local/bin/rm-personal-cloud

LABEL org.opencontainers.image.source=https://github.com/fpgaminer/rm-personal-cloud

ENTRYPOINT ["rm-personal-cloud"]
CMD ["--bind", "0.0.0.0", "--ssl-cert", "/ssl/cert.pem", "--ssl-key", "/ssl/key.pem", "--db", "/data/db.sqlite"]