# Small static image of the daemon.
#   docker build -t twitchminer .
#   docker run -d --name twitchminer -v twitchminer:/data -p 7878:7878 twitchminer
#   docker logs twitchminer          # shows the login code on first run
#   docker exec twitchminer twitchminer api-token
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p twitchminer

FROM alpine:3
RUN apk add --no-cache ca-certificates tzdata && adduser -D -u 1000 miner && mkdir /data && chown miner /data
COPY --from=build /src/target/release/twitchminer /usr/local/bin/twitchminer
USER miner
ENV TM_DATA_DIR=/data TM_API_BIND=0.0.0.0:7878
VOLUME /data
EXPOSE 7878
ENTRYPOINT ["twitchminer"]
CMD ["run"]
