# Build the admin webapp
FROM node:16 as webapp-builder

WORKDIR /usr/src/app
COPY admin-webapp/*.json ./
COPY admin-webapp/*.js ./
COPY admin-webapp/src ./src

RUN npm install
RUN npm run build-production


# Build the server, copying over the pre-built admin webapp
FROM rust:1.51 as builder

WORKDIR /usr/src/rm-personal-cloud
COPY src ./src
COPY Cargo.* ./
COPY schema.sql ./
COPY admin-webapp/dist/index.html ./admin-webapp/dist/index.html
COPY --from=webapp-builder /usr/src/app/dist/*.js ./admin-webapp/dist/
COPY --from=webapp-builder /usr/src/app/dist/*.map ./admin-webapp/dist/

RUN cargo install --path .


# Build the final image
FROM debian:buster-slim

COPY --from=builder /usr/local/cargo/bin/rm-personal-cloud /usr/local/bin/rm-personal-cloud

LABEL org.opencontainers.image.source=https://github.com/fpgaminer/rm-personal-cloud

ENTRYPOINT ["rm-personal-cloud"]
CMD ["--bind", "0.0.0.0", "--ssl-cert", "/ssl/cert.pem", "--ssl-key", "/ssl/key.pem", "--db", "/data/db.sqlite"]