FROM alpine:3.24.3@sha256:1b7232cda4ca3dc20b5d1247bdc9bfb631f2661d15d323bdd6fccc035ed1a9e7

RUN apk add --no-cache bash bind-tools docker-cli

WORKDIR /app
COPY . .

ENTRYPOINT ["/app/entrypoint.sh"]
