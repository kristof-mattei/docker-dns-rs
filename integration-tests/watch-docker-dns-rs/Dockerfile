FROM alpine:3.24.2@sha256:3cf95fe0816180395592b8373f3ec60663f076127617bbacb4eacf9667afe2e9

RUN apk add --no-cache bash bind-tools docker-cli

WORKDIR /app
COPY . .

ENTRYPOINT ["/app/entrypoint.sh"]
